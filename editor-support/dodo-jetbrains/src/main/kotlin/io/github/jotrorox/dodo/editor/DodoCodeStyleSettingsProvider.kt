package io.github.jotrorox.dodo.editor

import com.intellij.application.options.CodeStyleAbstractConfigurable
import com.intellij.application.options.CodeStyleAbstractPanel
import com.intellij.application.options.IndentOptionsEditor
import com.intellij.application.options.SmartIndentOptionsEditor
import com.intellij.application.options.TabbedLanguageCodeStylePanel
import com.intellij.lang.Language
import com.intellij.psi.codeStyle.CodeStyleConfigurable
import com.intellij.psi.codeStyle.CodeStyleSettings
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import com.intellij.psi.codeStyle.LanguageCodeStyleSettingsProvider
import io.github.jotrorox.dodo.lang.DodoLanguage

/** Four-space indentation, matching `dodo fmt`, which the language server applies on reformat. */
class DodoCodeStyleSettingsProvider : LanguageCodeStyleSettingsProvider() {
    override fun getLanguage(): Language = DodoLanguage

    override fun getCodeSample(settingsType: SettingsType): String = """
        package main

        fn main() -> i32 {
            for i in 0..3 {
                println(i)
            }
            return 0
        }
    """.trimIndent()

    override fun getIndentOptionsEditor(): IndentOptionsEditor = SmartIndentOptionsEditor()

    override fun customizeDefaults(commonSettings: CommonCodeStyleSettings, indentOptions: CommonCodeStyleSettings.IndentOptions) {
        indentOptions.INDENT_SIZE = 4
        indentOptions.CONTINUATION_INDENT_SIZE = 4
        indentOptions.TAB_SIZE = 4
        indentOptions.USE_TAB_CHARACTER = false
    }

    override fun createConfigurable(baseSettings: CodeStyleSettings, modelSettings: CodeStyleSettings): CodeStyleConfigurable =
        object : CodeStyleAbstractConfigurable(baseSettings, modelSettings, "Dodo") {
            override fun createPanel(settings: CodeStyleSettings): CodeStyleAbstractPanel =
                object : TabbedLanguageCodeStylePanel(DodoLanguage, currentSettings, settings) {
                    override fun initTabs(settings: CodeStyleSettings) {
                        addIndentOptionsTab(settings)
                    }
                }
        }
}
