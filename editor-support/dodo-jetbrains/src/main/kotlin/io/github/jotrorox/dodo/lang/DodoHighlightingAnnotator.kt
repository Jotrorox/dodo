package io.github.jotrorox.dodo.lang

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

/**
 * Colors identifiers by their surrounding tokens: declared names, types, constants,
 * packages, and calls. The rules and their precedence follow the VS Code grammar,
 * which also only looks at the current line.
 */
class DodoHighlightingAnnotator : Annotator {
    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        if (element.elementType != T.IDENTIFIER) return
        val key = classify(element) ?: return
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION).textAttributes(key).create()
    }

    companion object {
        private val DECLARED_TYPES = setOf("struct", "enum", "interface", "type")
        private val CALL_SUFFIX = Regex("""^[ \t]*(?:\(|::[ \t]*<|<[^<>\n]*>[ \t]*\()""")

        fun classify(identifier: PsiElement): TextAttributesKey? {
            val text = identifier.text
            val previous = sameLineNeighbor(identifier, forward = false)
            if (previous.elementType == T.KEYWORD) {
                when (previous!!.text) {
                    "fn" -> return DodoColors.FUNCTION_DECLARATION
                    in DECLARED_TYPES -> return DodoColors.TYPE_NAME
                    "package" -> return DodoColors.PACKAGE
                    "const" -> {
                        val next = sameLineNeighbor(identifier, forward = true)?.text
                        if (next != null && (next.startsWith(":") || next.startsWith("="))) return DodoColors.CONSTANT
                    }
                }
            }
            val rest = restOfLine(identifier)
            if (text == "asm" && rest.trimStart(' ', '\t').startsWith("(")) return DodoColors.KEYWORD
            if (text[0] in 'A'..'Z') return DodoColors.TYPE_NAME
            if (CALL_SUFFIX.containsMatchIn(rest)) return DodoColors.FUNCTION_CALL
            return null
        }

        private fun sameLineNeighbor(element: PsiElement, forward: Boolean): PsiElement? {
            var leaf = if (forward) PsiTreeUtil.nextLeaf(element) else PsiTreeUtil.prevLeaf(element)
            while (leaf != null && leaf.elementType == T.WHITE_SPACE) {
                if (leaf.textContains('\n')) return null
                leaf = if (forward) PsiTreeUtil.nextLeaf(leaf) else PsiTreeUtil.prevLeaf(leaf)
            }
            return leaf
        }

        private fun restOfLine(element: PsiElement): String {
            val text = element.containingFile.viewProvider.contents
            val start = element.textRange.endOffset
            var end = start
            while (end < text.length && text[end] != '\n') end++
            return text.subSequence(start, end).toString()
        }
    }
}
