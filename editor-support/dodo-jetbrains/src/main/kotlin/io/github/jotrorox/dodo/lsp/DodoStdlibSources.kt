package io.github.jotrorox.dodo.lsp

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.application.WriteAction
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspClient
import com.intellij.testFramework.LightVirtualFile
import io.github.jotrorox.dodo.lang.DodoFileType
import org.eclipse.lsp4j.DidCloseTextDocumentParams
import org.eclipse.lsp4j.DidOpenTextDocumentParams
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.TextDocumentIdentifier
import org.eclipse.lsp4j.TextDocumentItem
import java.util.concurrent.ConcurrentHashMap

/**
 * A read-only bundled library source, such as `dodo-stdlib:/std/math.dodo`,
 * whose text comes from the running compiler.
 */
class DodoStdlibFile(val uri: String, text: String) :
    LightVirtualFile(uri.substringAfterLast('/'), DodoFileType, text) {
    init {
        isWritable = false
    }

    override fun getPath(): String = uri.removePrefix(SCHEME)

    companion object {
        const val SCHEME = "dodo-stdlib:"
    }
}

/**
 * Bundled sources opened by navigation. The platform synchronizes only local project
 * files with the server, so these documents are opened and closed here, which
 * enables hover and navigation inside them.
 */
@Service(Service.Level.PROJECT)
class DodoStdlibSources(private val project: Project) {
    private val files = ConcurrentHashMap<String, DodoStdlibFile>()

    fun find(uri: String): DodoStdlibFile? {
        files[uri]?.let { return it }
        val server = DodoServers.client(project) ?: return null
        val text = fetch(server, uri) ?: return null
        return files.computeIfAbsent(uri) { DodoStdlibFile(uri, text) }
    }

    /** A new server may be a rebuilt compiler: refresh cached sources and reopen documents. */
    fun serverStarted() {
        val server = DodoServers.client(project) ?: return
        ApplicationManager.getApplication().executeOnPooledThread {
            for ((uri, file) in files) {
                val text = fetch(server, uri) ?: continue
                ApplicationManager.getApplication().invokeLater({ replaceText(file, text) }, project.disposed)
            }
            ApplicationManager.getApplication().invokeLater({
                val editors = FileEditorManager.getInstance(project)
                for (file in editors.openFiles) if (file is DodoStdlibFile) didOpen(server, file)
            }, project.disposed)
        }
    }

    private fun fetch(server: LspClient, uri: String): String? = try {
        server.sendRequestSync { (it as DodoLanguageServer).stdlibSource(DodoStdlibSourceParams(uri)) }
    } catch (error: Exception) {
        LOG.warn("Cannot load $uri", error)
        null
    }

    private fun replaceText(file: DodoStdlibFile, text: String) {
        if (file.content.toString() == text) return
        WriteAction.run<RuntimeException> {
            file.setContent(this, text, false)
            FileDocumentManager.getInstance().getCachedDocument(file)?.let { document ->
                document.setReadOnly(false)
                document.setText(text)
                document.setReadOnly(true)
            }
        }
    }

    fun didOpen(server: LspClient, file: DodoStdlibFile) {
        val text = ReadAction.computeBlocking<String, RuntimeException> { document(file)?.text ?: file.content.toString() }
        server.sendNotification {
            it.textDocumentService.didOpen(DidOpenTextDocumentParams(TextDocumentItem(file.uri, "dodo", 1, text)))
        }
    }

    fun didClose(server: LspClient, file: DodoStdlibFile) {
        server.sendNotification { it.textDocumentService.didClose(DidCloseTextDocumentParams(TextDocumentIdentifier(file.uri))) }
    }

    class EditorListener(private val project: Project) : FileEditorManagerListener {
        override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
            if (file !is DodoStdlibFile) return
            val server = DodoServers.client(project) ?: return
            getInstance(project).didOpen(server, file)
        }

        override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
            if (file !is DodoStdlibFile || source.isFileOpen(file)) return
            val server = DodoServers.client(project) ?: return
            getInstance(project).didClose(server, file)
        }
    }

    companion object {
        private val LOG = logger<DodoStdlibSources>()

        fun getInstance(project: Project): DodoStdlibSources = project.service()

        fun document(file: VirtualFile): Document? = FileDocumentManager.getInstance().getDocument(file)

        fun position(document: Document, offset: Int): Position {
            val line = document.getLineNumber(offset)
            return Position(line, offset - document.getLineStartOffset(line))
        }

        /** Converts an LSP position (UTF-16 columns, like Java strings) to a clamped document offset. */
        fun offset(document: Document, position: Position): Int {
            if (document.lineCount == 0) return 0
            val line = position.line.coerceIn(0, document.lineCount - 1)
            val range = TextRange(document.getLineStartOffset(line), document.getLineEndOffset(line))
            return (range.startOffset + position.character).coerceIn(range.startOffset, range.endOffset)
        }
    }
}
