package io.github.jotrorox.dodo.lsp

import com.intellij.execution.ExecutionException
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.process.ProcessEvent
import com.intellij.execution.process.ProcessListener
import com.intellij.execution.process.ProcessOutputType
import com.intellij.ide.trustedProjects.TrustedProjects
import com.intellij.openapi.fileTypes.FileTypeRegistry
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Key
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiFile
import com.intellij.platform.lsp.api.LspClient
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.platform.lsp.api.LspIntegrationProvider
import com.intellij.platform.lsp.api.LspServerListener
import com.intellij.platform.lsp.api.ProjectWideLspClientDescriptor
import com.intellij.platform.lsp.api.customization.LspCustomization
import com.intellij.platform.lsp.api.customization.LspFormattingCustomizer
import com.intellij.platform.lsp.api.customization.LspFormattingSupport
import com.intellij.platform.lsp.api.customization.LspRenameCustomizer
import com.intellij.platform.lsp.api.customization.LspRenameSupport
import com.intellij.platform.lsp.api.lsWidget.LspClientWidgetItem
import com.intellij.util.io.BaseDataReader
import com.intellij.util.io.BaseOutputReader
import io.github.jotrorox.dodo.lang.DodoFileType
import io.github.jotrorox.dodo.lang.DodoIcons
import io.github.jotrorox.dodo.settings.DodoConfigurable
import io.github.jotrorox.dodo.settings.DodoLocalSettings
import io.github.jotrorox.dodo.settings.DodoSettings
import org.eclipse.lsp4j.InitializeResult
import java.io.OutputStream

class DodoLspIntegrationProvider : LspIntegrationProvider {
    override fun fileOpened(project: Project, file: VirtualFile, clientStarter: LspIntegrationProvider.LspClientStarter) {
        if (FileTypeRegistry.getInstance().isFileOfType(file, DodoFileType) && DodoServers.shouldRun(project)) {
            clientStarter.ensureClientStarted(DodoLspClientDescriptor(project))
        }
    }

    override fun createWidgetItem(lspClient: LspClient, currentFile: VirtualFile?): LspClientWidgetItem =
        LspClientWidgetItem(lspClient, currentFile, DodoIcons.FILE, DodoConfigurable::class.java)
}

/** Server lifecycle shared by settings, actions, and trust changes. */
object DodoServers {
    fun shouldRun(project: Project): Boolean =
        DodoLocalSettings.getInstance(project).state.serverEnabled && TrustedProjects.isProjectTrusted(project)

    fun client(project: Project): LspClient? =
        LspClientManager.getInstance(project).getClients(DodoLspIntegrationProvider::class.java).firstOrNull()

    /** Restarts with the current settings so the server receives new initialization options. */
    fun restart(project: Project) {
        val manager = LspClientManager.getInstance(project)
        if (shouldRun(project)) {
            DodoServerOutput.getInstance(project).info("Restarting the Dodo language server.")
            manager.stopAndRestartClientsIfNeeded(DodoLspIntegrationProvider::class.java)
        } else {
            manager.stopClients(DodoLspIntegrationProvider::class.java)
        }
    }
}

class DodoLspClientDescriptor(project: Project) : ProjectWideLspClientDescriptor(project, "Dodo") {
    private val output = DodoServerOutput.getInstance(project)

    override fun isSupportedFile(file: VirtualFile): Boolean =
        FileTypeRegistry.getInstance().isFileOfType(file, DodoFileType)

    override fun createCommandLine(): GeneralCommandLine {
        val configured = DodoLocalSettings.getInstance(project).state.serverPath ?: DodoLocalSettings.DEFAULT_SERVER_PATH
        val executable = try {
            DodoServerPath.resolve(configured, project.basePath)
        } catch (error: ExecutionException) {
            reportStartFailure(error.message ?: error.toString())
            throw error
        }
        return GeneralCommandLine(executable.toString(), "lsp")
            .withParentEnvironmentType(GeneralCommandLine.ParentEnvironmentType.CONSOLE)
            .apply {
                project.basePath?.let { java.nio.file.Path.of(it) }?.takeIf { java.nio.file.Files.isDirectory(it) }
                    ?.let { withWorkingDirectory(it) }
            }
    }

    override fun startServerProcess(): OSProcessHandler {
        val commandLine = createCommandLine().withCharset(Charsets.UTF_8)
        output.info("Starting ${commandLine.commandLineString}")
        val tracer = DodoProtocolTracer({ DodoLocalSettings.getInstance(project).state.trace }) { text, kind -> output.log(text, kind) }
        val handler = try {
            object : OSProcessHandler(commandLine) {
                private val input by lazy { tracer.wrapOutgoing(super.getProcessInput()) }

                override fun getProcessInput(): OutputStream = input

                // Matches the platform's handler: raw, unsplit stdout for the JSON-RPC reader.
                override fun readerOptions(): BaseOutputReader.Options = object : BaseOutputReader.Options() {
                    override fun policy(): BaseDataReader.SleepingPolicy = forMostlySilentProcess().policy()

                    override fun splitToLines(): Boolean = false
                }
            }
        } catch (error: ExecutionException) {
            reportStartFailure(error.message ?: error.toString())
            throw error
        }
        handler.addProcessListener(object : ProcessListener {
            override fun onTextAvailable(event: ProcessEvent, outputType: Key<*>) {
                when {
                    ProcessOutputType.isStdout(outputType) -> tracer.incoming.feed(event.text.toByteArray(Charsets.UTF_8))
                    ProcessOutputType.isStderr(outputType) -> event.text.trimEnd().takeIf { it.isNotEmpty() }
                        ?.let { output.log(it, DodoServerOutput.Kind.SERVER) }
                }
            }

            override fun processTerminated(event: ProcessEvent) {
                // The platform ends the process right after `exit`, so its exit code is meaningless then.
                if (tracer.shutdownRequested) {
                    output.info("Language server stopped.")
                } else {
                    output.error("Language server exited unexpectedly with code ${event.exitCode}.")
                }
            }
        })
        return handler
    }

    private fun reportStartFailure(message: String) {
        output.error("Language server failed: $message")
        DodoNotifications.serverFailed(project, message)
    }

    override fun createInitializationOptions(): Any {
        val options = DodoSettings.getInstance(project).state
        return buildMap {
            put("checkMode", options.checkMode.value)
            options.target?.trim()?.takeIf { it.isNotEmpty() }?.let { put("target", it) }
            options.buildTarget?.trim()?.takeIf { it.isNotEmpty() }?.let { put("buildTarget", it) }
            options.manifestPath?.trim()?.takeIf { it.isNotEmpty() }?.let { put("manifestPath", it) }
        }
    }

    override val lsp4jServerClass: Class<out org.eclipse.lsp4j.services.LanguageServer> = DodoLanguageServer::class.java

    override fun getFileUri(file: VirtualFile): String =
        (file as? DodoStdlibFile)?.uri ?: super.getFileUri(file)

    override fun findFileByUri(fileUri: String): VirtualFile? =
        if (fileUri.startsWith(DodoStdlibFile.SCHEME)) {
            DodoStdlibSources.getInstance(project).find(fileUri)
        } else {
            super.findFileByUri(fileUri)
        }

    override val lspServerListener: LspServerListener = object : LspServerListener {
        override fun serverInitialized(params: InitializeResult) {
            val info = params.serverInfo
            output.info("Initialized ${info?.name ?: "dodo"} ${info?.version ?: ""}".trimEnd() + ".")
            DodoStdlibSources.getInstance(project).serverStarted()
        }
    }

    override val lspCustomization: LspCustomization = object : LspCustomization() {
        // The platform renames through the server only for plain-text files by default,
        // expecting custom languages to bring their own PSI rename. Dodo's PSI is flat.
        override val renameCustomizer: LspRenameCustomizer = object : LspRenameSupport() {
            override fun shouldRunRename(psiFile: PsiFile): Boolean = psiFile.virtualFile?.let(::isSupportedFile) == true
        }

        // `dodo fmt` style is canonical; there is no IDE formatter to defer to.
        override val formattingCustomizer: LspFormattingCustomizer = object : LspFormattingSupport() {
            override fun shouldFormatThisFileExclusivelyByServer(
                file: VirtualFile,
                ideCanFormatThisFileItself: Boolean,
                serverExplicitlyWantsToFormatThisFile: Boolean,
            ): Boolean = isSupportedFile(file)
        }
    }
}
