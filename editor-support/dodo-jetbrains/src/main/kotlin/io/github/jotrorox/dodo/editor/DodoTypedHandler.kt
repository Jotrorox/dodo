package io.github.jotrorox.dodo.editor

import com.intellij.codeInsight.editorActions.TypedHandlerDelegate
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.ex.EditorEx
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import io.github.jotrorox.dodo.lang.DodoFile

/** Outdents a closing bracket typed at the start of a line to its opening line's indent. */
class DodoTypedHandler : TypedHandlerDelegate() {
    override fun charTyped(c: Char, project: Project, editor: Editor, file: PsiFile): Result {
        if (file !is DodoFile || c !in "}])") return Result.CONTINUE
        val highlighter = (editor as? EditorEx)?.highlighter ?: return Result.CONTINUE
        val document = editor.document
        val bracket = editor.caretModel.offset - 1
        if (bracket < 0 || document.charsSequence[bracket] != c) return Result.CONTINUE
        val lineStart = document.getLineStartOffset(document.getLineNumber(bracket))
        val leading = document.charsSequence.subSequence(lineStart, bracket)
        if (leading.any { it != ' ' && it != '\t' }) return Result.CONTINUE
        val indent = DodoIndenter(document, project).indentAt(lineStart) { highlighter.createIterator(it) }
        if (indent != leading.toString()) {
            document.replaceString(lineStart, bracket, indent)
            editor.caretModel.moveToOffset(lineStart + indent.length + 1)
        }
        return Result.STOP
    }
}
