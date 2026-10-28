package io.github.jotrorox.dodo.lang

import com.intellij.lang.Language
import com.intellij.openapi.fileTypes.LanguageFileType
import com.intellij.openapi.util.IconLoader
import javax.swing.Icon

object DodoLanguage : Language("Dodo") {
    private fun readResolve(): Any = DodoLanguage

    override fun getDisplayName(): String = "Dodo"
}

object DodoIcons {
    @JvmField
    val FILE: Icon = IconLoader.getIcon("/icons/dodo.svg", DodoIcons::class.java)
}

object DodoFileType : LanguageFileType(DodoLanguage) {
    override fun getName(): String = "Dodo"

    override fun getDescription(): String = "Dodo source"

    override fun getDefaultExtension(): String = "dodo"

    override fun getIcon(): Icon = DodoIcons.FILE
}
