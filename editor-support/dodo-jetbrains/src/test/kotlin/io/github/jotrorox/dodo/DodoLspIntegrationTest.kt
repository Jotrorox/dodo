package io.github.jotrorox.dodo

import com.intellij.codeInsight.daemon.impl.HighlightInfo
import com.intellij.codeInsight.hint.ShowParameterInfoHandler
import com.intellij.codeInsight.template.impl.TemplateManagerImpl
import com.intellij.find.actions.FindUsagesAction
import com.intellij.find.usages.api.UsageOptions
import com.intellij.find.usages.impl.AllSearchOptions
import com.intellij.find.usages.impl.buildUsageViewQuery
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.openapi.actionSystem.impl.SimpleDataContext
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.application.WriteAction
import com.intellij.openapi.command.CommandProcessor
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.ex.util.EditorUtil
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.backend.documentation.DocumentationData
import com.intellij.platform.backend.documentation.DocumentationTargetProvider
import com.intellij.platform.lsp.api.LspServerState
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiManager
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.refactoring.rename.RenameHandlerRegistry
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.UsefulTestCase
import com.intellij.testFramework.builders.EmptyModuleFixtureBuilder
import com.intellij.testFramework.fixtures.CodeInsightTestFixture
import com.intellij.testFramework.fixtures.IdeaTestFixtureFactory
import com.intellij.testFramework.utils.parameterInfo.MockCreateParameterInfoContext
import io.github.jotrorox.dodo.lang.DodoLanguage
import io.github.jotrorox.dodo.lsp.DodoServerOutput
import io.github.jotrorox.dodo.lsp.DodoServers
import io.github.jotrorox.dodo.lsp.DodoStdlibFile
import io.github.jotrorox.dodo.lsp.DodoStdlibGotoDeclarationHandler
import io.github.jotrorox.dodo.settings.CheckMode
import io.github.jotrorox.dodo.settings.DodoLocalSettings
import io.github.jotrorox.dodo.settings.DodoSettings
import io.github.jotrorox.dodo.settings.TraceLevel
import java.io.File
import java.util.concurrent.Future

/**
 * Drives the plugin against the real `dodo lsp` server, like the VS Code
 * extension's integration test. Build the compiler (`cargo build`) first, or set
 * `DODO_TEST_SERVER` to a compiler path; the test is skipped without one.
 */
class DodoLspIntegrationTest : UsefulTestCase() {
    private lateinit var fixture: CodeInsightTestFixture
    private val compiler = File(System.getenv("DODO_TEST_SERVER") ?: "../../target/debug/dodo").absoluteFile
    private val project get() = fixture.project

    override fun setUp() {
        super.setUp()
        val factory = IdeaTestFixtureFactory.getFixtureFactory()
        val projectBuilder = factory.createFixtureBuilder(name)
        fixture = factory.createCodeInsightFixture(projectBuilder.fixture)
        projectBuilder.addModule(EmptyModuleFixtureBuilder::class.java).addContentRoot(fixture.tempDirPath)
        fixture.setUp()
        DodoLocalSettings.getInstance(project).state.serverPath = compiler.path
        DodoLocalSettings.getInstance(project).state.trace = TraceLevel.MESSAGES
    }

    override fun tearDown() {
        try {
            fixture.tearDown()
        } catch (error: Throwable) {
            addSuppressedException(error)
        } finally {
            super.tearDown()
        }
    }

    override fun runInDispatchThread(): Boolean = true

    // --- Helpers ---

    private fun <T : Any> eventually(description: String, timeoutMs: Long = 30_000, probe: () -> T?): T {
        val deadline = System.currentTimeMillis() + timeoutMs
        var failure: Throwable? = null
        while (true) {
            PlatformTestUtil.dispatchAllEventsInIdeEventQueue()
            try {
                probe()?.let { return it }
            } catch (error: Throwable) {
                failure = error
            }
            if (System.currentTimeMillis() > deadline) {
                throw AssertionError(
                    "Timed out waiting for $description (last error: $failure)\n" +
                        DodoServerOutput.getInstance(project).text().takeLast(3000),
                    failure,
                )
            }
            Thread.sleep(50)
        }
    }

    /** Runs [action] off the EDT, as the IDE does for documentation and navigation, while pumping events. */
    private fun <T> background(description: String, action: () -> T): T {
        val future: Future<T> = ApplicationManager.getApplication().executeOnPooledThread<T> { action() }
        eventually(description) { future.isDone.takeIf { it } }
        return future.get()
    }

    private fun errors(): List<HighlightInfo> =
        fixture.doHighlighting().filter { it.severity == HighlightSeverity.ERROR }

    private fun noErrors(description: String) = eventually(description) {
        val errors = errors()
        if (errors.isNotEmpty()) throw AssertionError(errors.joinToString { it.description ?: "" })
        true
    }

    private fun errorMatching(description: String, pattern: String): HighlightInfo =
        eventually(description) { errors().find { Regex(pattern).containsMatchIn(it.description ?: "") } }

    private fun replace(text: String, editor: Editor = fixture.editor) {
        WriteCommandAction.runWriteCommandAction(project) { editor.document.setText(text) }
        PsiDocumentManager.getInstance(project).commitAllDocuments()
    }

    private fun caretAt(text: String, delta: Int = 0) {
        val offset = fixture.editor.document.text.lastIndexOf(text)
        assertTrue("$text in ${fixture.editor.document.text}", offset >= 0)
        fixture.editor.caretModel.moveToOffset(offset + delta)
    }

    private fun hover(editor: Editor = fixture.editor, offset: Int = editor.caretModel.offset): String? = background("hover") {
        ReadAction.computeBlocking<String?, RuntimeException> {
            val file = PsiDocumentManager.getInstance(project).getPsiFile(editor.document)!!
            DocumentationTargetProvider.EP_NAME.extensionList
                .flatMap { it.documentationTargets(file, offset) }
                .firstNotNullOfOrNull { (it.computeDocumentation() as? DocumentationData)?.html }
        }
    }

    private fun writeFile(path: String, text: String): VirtualFile = WriteAction.computeAndWait<VirtualFile, Exception> {
        val root = VfsUtil.findFile(java.nio.file.Path.of(fixture.tempDirPath), true)!!
        val file = VfsUtil.createDirectoryIfMissing(root, path.substringBeforeLast('/', "")) ?.let {
            it.findChild(path.substringAfterLast('/')) ?: it.createChildData(this, path.substringAfterLast('/'))
        }!!
        VfsUtil.saveText(file, text)
        file
    }

    private fun waitForServer() =
        eventually("server start") { DodoServers.client(project)?.takeIf { it.state == LspServerState.Running } }

    private fun restart() {
        val previous = DodoServers.client(project)
        DodoServers.restart(project)
        eventually("restarted server") { DodoServers.client(project)?.takeIf { it !== previous && it.state == LspServerState.Running } }
    }

    fun testLanguageServerFeatures() {
        if (!compiler.canExecute()) {
            println("Skipping: no Dodo compiler at $compiler")
            return
        }
        val source = "package editor_test\nfn add(left: i32, right: i32) -> i32 { return left + right }\n" +
            "fn main() -> i32 {\nlet answer = add(20, 22)\nreturn answer\n}\n"
        val main = fixture.addFileToProject("main.dodo", source).virtualFile
        fixture.configureFromExistingVirtualFile(main)
        waitForServer()

        // Hover, completion, navigation, rename, and parameter info.
        caretAt("answer", 2)
        assertTrue(eventually("hover from dodo lsp") { hover()?.takeIf { it.contains("i32") } }.isNotEmpty())

        caretAt("answer")
        val completions = eventually("completion") { fixture.completeBasic()?.map { it.lookupString }?.takeIf { "answer" in it } }
        assertTrue(completions.toString(), "add" in completions)
        fixture.lookup?.hideLookup(true)
        assertEquals(source, fixture.editor.document.text)

        caretAt("add(", 1)
        fixture.performEditorAction(IdeActions.ACTION_GOTO_DECLARATION)
        assertEquals(1, fixture.editor.document.getLineNumber(fixture.editor.caretModel.offset))

        caretAt("answer", 2)
        val searchTarget = FindUsagesAction.SEARCH_TARGETS.getData(EditorUtil.getEditorDataContext(fixture.editor))?.singleOrNull()
        assertNotNull("find usages target", searchTarget)
        val usages = background("find usages") {
            val options = AllSearchOptions(UsageOptions.createOptions(GlobalSearchScope.projectScope(project)), false)
            buildUsageViewQuery(project, searchTarget!!, options).findAll()
        }
        assertTrue(usages.toString(), usages.size >= 2)

        caretAt("22)", 1)
        val context = MockCreateParameterInfoContext(fixture.editor, fixture.file)
        val handler = ShowParameterInfoHandler.getHandlers(project, DodoLanguage).firstNotNullOfOrNull { handler ->
            handler.takeIf { background("parameter info") { ReadAction.computeBlocking<Any?, RuntimeException> { it.findElementForParameterInfo(context) } } != null }
        }
        assertNotNull("parameter info handler", handler)
        assertTrue(context.itemsToShow.orEmpty().joinToString(), context.itemsToShow.orEmpty().isNotEmpty())

        caretAt("answer", 2)
        // The data an editor provides; the fixture's action context lacks the virtual file.
        val renameContext = SimpleDataContext.builder()
            .add(CommonDataKeys.PROJECT, project)
            .add(CommonDataKeys.EDITOR, fixture.editor)
            .add(CommonDataKeys.VIRTUAL_FILE, main)
            .add(CommonDataKeys.PSI_FILE, fixture.file)
            .build()
        val renameHandler = RenameHandlerRegistry.getInstance().getRenameHandler(renameContext)
        assertNotNull("rename handler", renameHandler)
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
        renameHandler!!.invoke(project, fixture.editor, fixture.file, renameContext)
        // In-place rename: type over the name, then finish to send the rename request.
        val template = eventually("in-place rename") { TemplateManagerImpl.getTemplateState(fixture.editor) }
        fixture.type("total")
        CommandProcessor.getInstance().executeCommand(project, { template.gotoEnd(false) }, "Finish Rename", null)
        eventually("rename") { fixture.editor.document.text.takeIf { it.contains("let total = add") && it.contains("return total") } }

        // Diagnostics follow unsaved edits and leave the disk untouched.
        replace(source.replace("return answer", "return missing_name"))
        errorMatching("unsaved diagnostics", "missing_name")
        assertEquals(source, File(main.path).readText())
        replace(source)
        noErrors("cleared diagnostics")

        // Inferred type inlay hints.
        val hintSource = "package editor_test\nfn main() -> i32 {\nlet answer = 42i32\nlet annotated: i32 = 7\nreturn answer + annotated\n}\n"
        replace(hintSource)
        val hintOffset = hintSource.indexOf("answer") + "answer".length
        fun hintOffsets() = fixture.doHighlighting().let {
            fixture.editor.inlayModel.getInlineElementsInRange(0, fixture.editor.document.textLength).map { inlay -> inlay.offset }
        }
        eventually("inferred type inlay hint") { hintOffsets().takeIf { it == listOf(hintOffset) } }
        replace(hintSource.replace("let answer =", "let answer: i32 ="))
        eventually("inlay hints update for unsaved annotations") { hintOffsets().isEmpty().takeIf { it } }

        // Quick fixes from the server, including undo.
        replace("package editor_test\nfn main() -> i32 {\nlet count = 1i32\ncount = 2\nreturn count\n}\n")
        val immutable = errorMatching("immutable assignment diagnostic", "immutable binding")
        fixture.editor.caretModel.moveToOffset(immutable.startOffset)
        val mutableFix = eventually("make mutable quick fix") { fixture.availableIntentions.find { it.text == "Make `count` mutable" } }
        fixture.launchAction(mutableFix)
        assertTrue(fixture.editor.document.text, fixture.editor.document.text.contains("\ncount := 1i32\n"))
        noErrors("mutable quick fix clears diagnostics")

        val math = writeFile("math.dodo", "package math\npub fn answer() -> i32 { return 42 }\n")
        replace("package editor_test\nfn main() -> i32 { return math.answer() }\n")
        val missingImport = errorMatching("missing import diagnostic", "cannot resolve the receiver type|unknown.*math")
        fixture.editor.caretModel.moveToOffset(missingImport.startOffset)
        val importFix = eventually("missing import quick fix") { fixture.availableIntentions.find { it.text == "Import `math`" } }
        fixture.launchAction(importFix)
        assertTrue(fixture.editor.document.text, fixture.editor.document.text.contains("import \"math\""))
        noErrors("import quick fix clears diagnostics")
        WriteAction.runAndWait<Exception> { math.delete(this) }

        // Formatting is done by the server.
        replace(source)
        fixture.performEditorAction(IdeActions.ACTION_EDITOR_REFORMAT)
        eventually("formatting") { fixture.editor.document.text.takeIf { it.contains("\n    let answer") } }

        // Restart, then bundled library sources.
        restart()
        caretAt("answer", 2)
        eventually("hover after restart") { hover() }

        replace("package editor_test\nimport \"std/math\"\nfn main() { _ = math.abs(-1.0) }\n")
        noErrors("library import")
        caretAt("abs", 1)
        fixture.performEditorAction(IdeActions.ACTION_GOTO_DECLARATION)
        val libraryEditor = eventually("bundled definition") {
            FileEditorManager.getInstance(project).selectedTextEditor?.takeIf { it.virtualFile is DodoStdlibFile }
        }
        val libraryFile = libraryEditor.virtualFile as DodoStdlibFile
        assertTrue(libraryFile.uri, libraryFile.uri.startsWith("dodo-stdlib:/"))
        assertFalse(libraryFile.isWritable)
        val libraryDocument = libraryEditor.document
        val caretLine = libraryDocument.getLineNumber(libraryEditor.caretModel.offset)
        assertTrue(libraryDocument.getText(com.intellij.openapi.util.TextRange(libraryDocument.getLineStartOffset(caretLine), libraryDocument.getLineEndOffset(caretLine))).contains("pub fn abs"))

        val libraryOffset = libraryDocument.text.indexOf("from_bits(0x7ff")
        assertTrue(libraryOffset >= 0)
        val libraryPsi = PsiManager.getInstance(project).findFile(libraryFile)!!
        val inside = eventually("navigation inside bundled source") {
            background("bundled navigation") {
                ReadAction.computeBlocking<Array<com.intellij.psi.PsiElement>?, RuntimeException> {
                    DodoStdlibGotoDeclarationHandler().getGotoDeclarationTargets(libraryPsi.findElementAt(libraryOffset), libraryOffset, libraryEditor)
                }
            }?.takeIf { it.isNotEmpty() }
        }
        assertEquals(libraryFile, inside.first().containingFile.virtualFile)
        restart()
        eventually("bundled hover after restart") { hover(libraryEditor, libraryOffset) }
        FileEditorManager.getInstance(project).closeFile(libraryFile)
        fixture.openFileInEditor(main)

        // Changing an unopened import on disk refreshes the open caller.
        val dependencySource = "package lib\npub fn value() -> i32 { return 7 }\n"
        val dependency = writeFile("lib/value.dodo", dependencySource)
        replace("package editor_test\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n")
        noErrors("import resolves")
        caretAt("value", 1)
        fixture.performEditorAction(IdeActions.ACTION_GOTO_DECLARATION)
        eventually("import navigation") { FileEditorManager.getInstance(project).selectedFiles.firstOrNull()?.takeIf { it == dependency } }
        fixture.openFileInEditor(main)
        WriteAction.runAndWait<Exception> { dependency.delete(this) }
        errorMatching("watched dependency deletion", "no .dodo|cannot resolve")
        writeFile("lib/value.dodo", dependencySource)
        noErrors("watched dependency recreation")

        // Project settings reach the server after a restart.
        DodoSettings.getInstance(project).state.checkMode = CheckMode.PACKAGE
        restart()
        writeFile("helper.dodo", "package editor_test\npub fn helper() -> i32 { return 42 }\n")
        replace("package editor_test\nfn main() -> i32 { return helper() }\n")
        noErrors("package mode resolves siblings")

        DodoSettings.getInstance(project).state.target = "wasm32-unknown-unknown"
        restart()
        replace("package editor_test\nfn main() -> i32 {\n    value: usize = 4294967296\n    return 0\n}\n")
        errorMatching("32-bit target diagnostics", "usize|range|fit")

        // Disabling stops the server; enabling starts it again.
        DodoLocalSettings.getInstance(project).state.serverEnabled = false
        DodoServers.restart(project)
        eventually("disabled server") { (DodoServers.client(project) == null).takeIf { it } }
        DodoLocalSettings.getInstance(project).state.serverEnabled = true
        restart()
    }
}
