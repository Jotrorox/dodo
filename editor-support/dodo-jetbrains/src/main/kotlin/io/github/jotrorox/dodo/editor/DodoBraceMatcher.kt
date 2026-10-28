package io.github.jotrorox.dodo.editor

import com.intellij.lang.BracePair
import com.intellij.lang.PairedBraceMatcher
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IElementType
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

class DodoBraceMatcher : PairedBraceMatcher {
    override fun getPairs(): Array<BracePair> = PAIRS

    // Closing pairs are inserted before whitespace, comments, separators, and closers.
    override fun isPairedBracesAllowedBeforeType(lbraceType: IElementType, contextType: IElementType?): Boolean =
        contextType == null || contextType in ALLOWED_BEFORE

    override fun getCodeConstructStart(file: PsiFile?, openingBraceOffset: Int): Int = openingBraceOffset

    companion object {
        private val PAIRS = arrayOf(
            BracePair(T.LBRACE, T.RBRACE, true),
            BracePair(T.LBRACKET, T.RBRACKET, false),
            BracePair(T.LPAREN, T.RPAREN, false),
        )
        private val ALLOWED_BEFORE = setOf(
            T.WHITE_SPACE, T.LINE_COMMENT, T.RBRACE, T.RBRACKET, T.RPAREN, T.COMMA, T.SEMICOLON, T.COLON, T.DOT,
        )
    }
}
