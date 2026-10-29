package io.github.jotrorox.dodo.editor

import com.intellij.application.options.CodeStyle
import com.intellij.lang.Language
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.ex.EditorEx
import com.intellij.openapi.editor.highlighter.HighlighterIterator
import com.intellij.openapi.project.Project
import com.intellij.psi.codeStyle.lineIndent.LineIndentProvider
import io.github.jotrorox.dodo.lang.DodoLanguage
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

/**
 * Indents new lines without a formatting model, like VS Code's indentation rules:
 * one level deeper after an opening bracket, and a closing bracket aligns with the
 * line of its opening bracket. The language server's formatter remains canonical.
 */
class DodoLineIndentProvider : LineIndentProvider {
    override fun getLineIndent(project: Project, editor: Editor, language: Language?, offset: Int): String? {
        val highlighter = (editor as? EditorEx)?.highlighter ?: return null
        return DodoIndenter(editor.document, project).indentAt(offset) { highlighter.createIterator(it) }
    }

    override fun isSuitableFor(language: Language?): Boolean = language is DodoLanguage
}

internal class DodoIndenter(private val document: Document, private val project: Project) {
    fun indentAt(offset: Int, iteratorAt: (Int) -> HighlighterIterator): String {
        val text = document.charsSequence
        val line = document.getLineNumber(offset)
        val lineStart = document.getLineStartOffset(line)
        val lineEnd = document.getLineEndOffset(line)

        var firstOnLine = offset
        while (firstOnLine < lineEnd && (text[firstOnLine] == ' ' || text[firstOnLine] == '\t')) firstOnLine++
        if (firstOnLine < lineEnd) {
            val iterator = iteratorAt(firstOnLine)
            if (!iterator.atEnd() && iterator.start == firstOnLine && iterator.tokenType in T.CLOSE_BRACKETS) {
                matchingOpen(iterator)?.let { return lineIndent(document.getLineNumber(it)) }
            }
        }

        if (lineStart == 0) return ""
        val iterator = iteratorAt(lineStart - 1)
        while (!iterator.atEnd() && (iterator.tokenType == T.WHITE_SPACE || iterator.tokenType in T.COMMENTS)) {
            iterator.retreat()
        }
        if (iterator.atEnd()) return ""
        val base = lineIndent(document.getLineNumber(iterator.start))
        return if (iterator.tokenType in T.OPEN_BRACKETS) base + indentUnit() else base
    }

    /** Scans backward from a closing bracket to the offset of the bracket it closes. */
    private fun matchingOpen(iterator: HighlighterIterator): Int? {
        var depth = 0
        iterator.retreat()
        while (!iterator.atEnd()) {
            when (iterator.tokenType) {
                in T.CLOSE_BRACKETS -> depth++
                in T.OPEN_BRACKETS -> if (depth-- == 0) return iterator.start
            }
            iterator.retreat()
        }
        return null
    }

    private fun lineIndent(line: Int): String {
        val text = document.charsSequence
        val start = document.getLineStartOffset(line)
        var end = start
        while (end < text.length && (text[end] == ' ' || text[end] == '\t')) end++
        return text.subSequence(start, end).toString()
    }

    private fun indentUnit(): String {
        val options = CodeStyle.getIndentOptions(project, document)
        return if (options.USE_TAB_CHARACTER) "\t" else " ".repeat(options.INDENT_SIZE)
    }
}
