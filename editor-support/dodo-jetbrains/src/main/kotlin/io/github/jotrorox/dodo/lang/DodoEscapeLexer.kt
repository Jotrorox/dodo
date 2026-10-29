package io.github.jotrorox.dodo.lang

import com.intellij.lexer.LexerBase
import com.intellij.psi.tree.IElementType
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

/**
 * Splits one string or byte literal into plain text and escape sequences.
 * `\n \r \t \0 \\ \" \'` and `\xHH` are valid everywhere; `\u{1-6 hex}` only in
 * text strings. Any other backslash sequence is an invalid escape.
 */
class DodoEscapeLexer(private val literalType: IElementType) : LexerBase() {
    private var buffer: CharSequence = ""
    private var bufferEnd = 0
    private var tokenStart = 0
    private var tokenEnd = 0
    private var tokenType: IElementType? = null

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        bufferEnd = endOffset
        tokenEnd = startOffset
        advance()
    }

    override fun getState(): Int = 0

    override fun getTokenType(): IElementType? = tokenType

    override fun getTokenStart(): Int = tokenStart

    override fun getTokenEnd(): Int = tokenEnd

    override fun getBufferSequence(): CharSequence = buffer

    override fun getBufferEnd(): Int = bufferEnd

    override fun advance() {
        tokenStart = tokenEnd
        if (tokenStart >= bufferEnd) {
            tokenType = null
            return
        }
        if (buffer[tokenStart] != '\\') {
            var position = tokenStart
            while (position < bufferEnd && buffer[position] != '\\') position++
            tokenEnd = position
            tokenType = literalType
            return
        }
        val valid = validEscapeLength(tokenStart)
        if (valid > 0) {
            tokenEnd = tokenStart + valid
            tokenType = T.VALID_ESCAPE
        } else {
            tokenEnd = (tokenStart + 2).coerceAtMost(bufferEnd)
            tokenType = T.INVALID_ESCAPE
        }
    }

    private fun validEscapeLength(start: Int): Int {
        val next = at(start + 1)
        return when {
            next in "nrt0\\\"'" -> 2
            next == 'x' && isHex(at(start + 2)) && isHex(at(start + 3)) -> 4
            next == 'u' && literalType == T.STRING && at(start + 2) == '{' -> {
                var position = start + 3
                while (isHex(at(position)) && position - (start + 3) < 6) position++
                if (position > start + 3 && at(position) == '}') position + 1 - start else 0
            }
            else -> 0
        }
    }

    private fun at(index: Int): Char = if (index < bufferEnd) buffer[index] else '\u0000'

    private fun isHex(c: Char): Boolean = c in '0'..'9' || c in 'a'..'f' || c in 'A'..'F'
}
