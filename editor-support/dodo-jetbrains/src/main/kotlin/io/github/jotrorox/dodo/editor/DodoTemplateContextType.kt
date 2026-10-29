package io.github.jotrorox.dodo.editor

import com.intellij.codeInsight.template.TemplateActionContext
import com.intellij.codeInsight.template.TemplateContextType
import com.intellij.psi.util.elementType
import io.github.jotrorox.dodo.lang.DodoFile
import io.github.jotrorox.dodo.lang.DodoTokenTypes

/** Dodo code outside comments and literals, where snippets make sense. */
class DodoTemplateContextType : TemplateContextType("Dodo") {
    override fun isInContext(templateActionContext: TemplateActionContext): Boolean {
        val file = templateActionContext.file
        if (file !is DodoFile) return false
        val offset = templateActionContext.startOffset
        val element = file.findElementAt(offset) ?: file.findElementAt(offset - 1) ?: return true
        val type = element.elementType
        // At a literal's or comment's end, the caret is outside it only if it is closed.
        if (offset == element.textRange.startOffset) return true
        return type !in DodoTokenTypes.COMMENTS && type !in DodoTokenTypes.STRINGS
    }
}
