package io.github.jotrorox.dodo.editor

import com.intellij.codeInsight.editorActions.SimpleTokenSetQuoteHandler
import com.intellij.openapi.editor.highlighter.HighlighterIterator
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

/**
 * Pairs `"`, and `'` after a `b` byte-literal prefix, outside strings and comments.
 *
 * The highlighter splits literals around escape sequences, so one literal can span
 * several iterator tokens; only the first starts with the opening quote.
 */
class DodoQuoteHandler : SimpleTokenSetQuoteHandler(T.STRING, T.BYTE_STRING, T.BYTE_CHAR) {
    override fun isOpeningQuote(iterator: HighlighterIterator, offset: Int): Boolean =
        myLiteralTokenSet.contains(iterator.tokenType) && isOpeningPosition(iterator, offset)

    override fun isClosingQuote(iterator: HighlighterIterator, offset: Int): Boolean {
        if (!myLiteralTokenSet.contains(iterator.tokenType)) return false
        val chars = iterator.document.charsSequence
        return offset == iterator.end - 1 && isQuote(chars[offset]) && !isOpeningPosition(iterator, offset)
    }

    override fun isNonClosedLiteral(iterator: HighlighterIterator, chars: CharSequence): Boolean {
        if (isEscape(neighborType(iterator, forward = true))) return false
        val minimum = if (isFirstPiece(iterator)) prefixLength(iterator) + 2 else 1
        return iterator.end - iterator.start < minimum || !isQuote(chars[iterator.end - 1])
    }

    private fun isOpeningPosition(iterator: HighlighterIterator, offset: Int): Boolean =
        isFirstPiece(iterator) && offset == iterator.start + prefixLength(iterator)

    private fun isFirstPiece(iterator: HighlighterIterator): Boolean =
        !isEscape(neighborType(iterator, forward = false))

    private fun neighborType(iterator: HighlighterIterator, forward: Boolean): Any? {
        if (forward) iterator.advance() else iterator.retreat()
        val type = if (iterator.atEnd()) null else iterator.tokenType
        if (forward) iterator.retreat() else iterator.advance()
        return type
    }

    private fun isEscape(type: Any?): Boolean = type == T.VALID_ESCAPE || type == T.INVALID_ESCAPE

    private fun isQuote(c: Char): Boolean = c == '"' || c == '\''

    private fun prefixLength(iterator: HighlighterIterator): Int =
        if (iterator.tokenType == T.STRING) 0 else 1
}
