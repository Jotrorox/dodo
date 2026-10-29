package io.github.jotrorox.dodo

import com.intellij.codeInsight.CodeInsightSettings
import com.intellij.codeInsight.template.impl.TemplateSettings
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.wm.ToolWindowEP
import com.intellij.testFramework.EditorTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.util.ui.UIUtil
import io.github.jotrorox.dodo.lang.DodoColors
import io.github.jotrorox.dodo.lang.DodoFileType
import io.github.jotrorox.dodo.settings.DodoConfigurable
import io.github.jotrorox.dodo.settings.DodoLocalSettings
import java.io.File
import javax.swing.JCheckBox

class DodoEditorTest : BasePlatformTestCase() {
    private fun configure(text: String) {
        myFixture.configureByText(DodoFileType, text)
    }

    override fun tearDown() {
        try {
            // CLion turns on documentation popups once during its first highlighting pass,
            // which the test framework reports as settings damage.
            CodeInsightSettings.getInstance().AUTO_POPUP_JAVADOC_INFO = CodeInsightSettings().AUTO_POPUP_JAVADOC_INFO
        } finally {
            super.tearDown()
        }
    }

    // --- Contextual highlighting (cases from the VS Code grammar tests) ---

    private fun annotations(text: String): Map<String, TextAttributesKey> {
        configure(text)
        val document = myFixture.editor.document
        return myFixture.doHighlighting()
            .filter { it.forcedTextAttributesKey != null }
            .associate { document.getText(com.intellij.openapi.util.TextRange(it.startOffset, it.endOffset)) to it.forcedTextAttributesKey!! }
    }

    private fun assertColor(line: String, token: String, key: TextAttributesKey?) {
        assertEquals("$token in $line", key, annotations(line)[token])
    }

    fun testDeclarationsAndCalls() {
        assertColor("package hello", "hello", DodoColors.PACKAGE)
        assertColor("pub struct Reading {", "Reading", DodoColors.TYPE_NAME)
        assertColor("pub interface Sensor: Named {", "Sensor", DodoColors.TYPE_NAME)
        assertColor("pub interface Sensor: Named {", "Named", DodoColors.TYPE_NAME)
        assertColor("pub type Pair<T> = (T, T)", "Pair", DodoColors.TYPE_NAME)
        assertColor("pub fn view(&self) -> &i32 from(self) {", "view", DodoColors.FUNCTION_DECLARATION)
        assertColor("fn Build() {}", "Build", DodoColors.FUNCTION_DECLARATION)
        assertColor("const LIMIT: usize = 4", "LIMIT", DodoColors.CONSTANT)
        assertColor("const LIMIT = 4", "LIMIT", DodoColors.CONSTANT)
        assertColor("value := asm(\"mrs {0}, primask\", out(reg) u32)", "asm", DodoColors.KEYWORD)
        assertColor("identity::<Option<i32>>(some(42))", "identity", DodoColors.FUNCTION_CALL)
        assertColor("sort_by(&mut v, fn(a, b) { *a - *b })", "sort_by", DodoColors.FUNCTION_CALL)
        assertColor("x := parse<i32>(text)", "parse", DodoColors.FUNCTION_CALL)
        assertColor("x := console.println (text)", "println", DodoColors.FUNCTION_CALL)
        assertColor("x := value\n(other)", "value", null)
        assertColor("fn\nmain() {}", "main", DodoColors.FUNCTION_CALL)
        assertColor("let asm = 1", "asm", null)
    }

    fun testColorSettingsPageDemoUsesKnownTags() {
        val page = io.github.jotrorox.dodo.editor.DodoColorSettingsPage()
        val tags = Regex("<(\\w+)>").findAll(page.demoText).map { it.groupValues[1] }.toSet()
        assertEquals(page.additionalHighlightingTagToDescriptorMap.keys, tags)
    }

    // --- Registrations ---

    fun testActionsSettingsAndToolWindowAreRegistered() {
        assertNotNull(ActionManager.getInstance().getAction("Dodo.RestartServer"))
        assertNotNull(ActionManager.getInstance().getAction("Dodo.ShowOutput"))
        assertTrue(ToolWindowEP.EP_NAME.extensionList.any { it.id == "Dodo Language Server" })

        // The settings page binds the settings stores and applies edits to them.
        val configurable = DodoConfigurable(project)
        try {
            val panel = configurable.createComponent()!!
            assertFalse(configurable.isModified)
            UIUtil.findComponentsOfType(panel, JCheckBox::class.java).single().isSelected = false
            assertTrue(configurable.isModified)
            configurable.apply()
            assertFalse(DodoLocalSettings.getInstance(project).state.serverEnabled)
        } finally {
            configurable.disposeUIResources()
            DodoLocalSettings.getInstance(project).state.serverEnabled = true
        }
    }

    // --- Comments, pairs, and indentation ---

    fun testLineCommentToggle() {
        configure("fn main() {<caret>}\n")
        myFixture.performEditorAction(IdeActions.ACTION_COMMENT_LINE)
        myFixture.checkResult("//fn main() {}\n")
    }

    fun testBracketAndQuotePairs() {
        configure("x := <caret>")
        myFixture.type("(")
        myFixture.checkResult("x := (<caret>)")
        myFixture.type("[")
        myFixture.checkResult("x := ([<caret>])")
        configure("x := <caret>")
        myFixture.type("\"")
        myFixture.checkResult("x := \"<caret>\"")
        myFixture.type("a\"")
        myFixture.checkResult("x := \"a\"<caret>")
        configure("x := <caret>")
        myFixture.type("b'")
        myFixture.checkResult("x := b'<caret>'")
        myFixture.type("\\n'")
        myFixture.checkResult("x := b'\\n'<caret>")
    }

    fun testNoQuotePairInsideComments() {
        configure("// say <caret>")
        myFixture.type("\"")
        myFixture.checkResult("// say \"<caret>")
    }

    fun testEnterIndentsAfterOpeningBracket() {
        configure("fn main() {<caret>")
        myFixture.type("\n")
        myFixture.checkResult("fn main() {\n    <caret>\n}")
    }

    fun testEnterBetweenBraces() {
        configure("fn main() {\n    if ok {<caret>}\n}")
        myFixture.type("\n")
        myFixture.checkResult("fn main() {\n    if ok {\n        <caret>\n    }\n}")
    }

    fun testEnterKeepsIndentAndIgnoresComments() {
        configure("fn main() {\n    let x = 1 // trailing {<caret>\n}")
        myFixture.type("\n")
        myFixture.checkResult("fn main() {\n    let x = 1 // trailing {\n    <caret>\n}")
    }

    fun testClosingBracketOutdents() {
        configure("fn main() {\n    call(\n        1,\n        <caret>")
        myFixture.type(")")
        myFixture.checkResult("fn main() {\n    call(\n        1,\n    )<caret>")
        myFixture.type("\n}")
        myFixture.checkResult("fn main() {\n    call(\n        1,\n    )\n}<caret>")
    }

    // --- Folding ---

    fun testFolding() {
        myFixture.configureByText(
            DodoFileType,
            """
            // region Helpers
            // First line
            // second line
            fn main() {
                call(
                    1,
                )
                one_line { }
            }
            // endregion
            """.trimIndent(),
        )
        EditorTestUtil.buildInitialFoldingsInBackground(myFixture.editor)
        val regions = myFixture.editor.foldingModel.allFoldRegions.map { it.placeholderText to myFixture.editor.document.getText(it.textRange) }
        assertTrue(regions.toString(), regions.any { it.first == "Helpers" && it.second.endsWith("// endregion") })
        assertTrue(regions.toString(), regions.any { it.first == "//..." && it.second == "// First line\n// second line" })
        assertTrue(regions.toString(), regions.any { it.first == "{...}" && it.second.startsWith("{\n    call(") })
        assertTrue(regions.toString(), regions.any { it.first == "(...)" })
        assertEquals(regions.toString(), 4, regions.size)
    }

    // --- Live templates ---

    fun testEveryVsCodeSnippetIsALiveTemplate() {
        val snippets = com.google.gson.JsonParser.parseString(File("../dodo-vscode/snippets/dodo.json").readText()).asJsonObject
        for ((name, snippet) in snippets.entrySet()) {
            val prefix = snippet.asJsonObject["prefix"].asString
            assertNotNull("$name ($prefix)", TemplateSettings.getInstance().getTemplate(prefix, "Dodo"))
        }
    }

    fun testFunctionTemplateExpands() {
        configure("fn<caret>")
        myFixture.performEditorAction(IdeActions.ACTION_EXPAND_LIVE_TEMPLATE_BY_TAB)
        myFixture.type("answer")
        myFixture.performEditorAction(IdeActions.ACTION_EDITOR_NEXT_TEMPLATE_VARIABLE)
        myFixture.performEditorAction(IdeActions.ACTION_EDITOR_NEXT_TEMPLATE_VARIABLE)
        myFixture.type("i32")
        myFixture.performEditorAction(IdeActions.ACTION_EDITOR_NEXT_TEMPLATE_VARIABLE)
        myFixture.type("return 42")
        myFixture.checkResult("fn answer() -> i32 {\n    return 42<caret>\n}")
    }

    fun testTemplatesAreNotOfferedInComments() {
        configure("// fn<caret>")
        myFixture.performEditorAction(IdeActions.ACTION_EXPAND_LIVE_TEMPLATE_BY_TAB)
        assertFalse(myFixture.editor.document.text.contains("->"))
    }
}
