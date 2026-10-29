package io.github.jotrorox.dodo.lang

import com.intellij.lexer.LexerBase
import com.intellij.psi.tree.IElementType
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

/**
 * Tokenizes Dodo source like the compiler's lexer (`src/lexer.rs`), but never fails:
 * unterminated literals end at the line break and unexpected characters become
 * [T.BAD_CHARACTER], so highlighting stays stable while the user types.
 *
 * The only state is whether the previous significant token was `.`, which makes
 * `pair.0.1` two tuple indices instead of a float, exactly as in the compiler.
 */
class DodoLexer : LexerBase() {
    private var buffer: CharSequence = ""
    private var bufferEnd = 0
    private var tokenStart = 0
    private var tokenEnd = 0
    private var tokenType: IElementType? = null
    private var stateBefore = NORMAL
    private var stateAfter = NORMAL

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        bufferEnd = endOffset
        tokenEnd = startOffset
        stateAfter = initialState
        advance()
    }

    override fun getState(): Int = stateBefore

    override fun getTokenType(): IElementType? = tokenType

    override fun getTokenStart(): Int = tokenStart

    override fun getTokenEnd(): Int = tokenEnd

    override fun getBufferSequence(): CharSequence = buffer

    override fun getBufferEnd(): Int = bufferEnd

    override fun advance() {
        stateBefore = stateAfter
        tokenStart = tokenEnd
        if (tokenStart >= bufferEnd) {
            tokenType = null
            return
        }
        var position = tokenStart
        val c = buffer[position]
        val type: IElementType
        when {
            c == ' ' || c == '\t' || c == '\r' || c == '\n' -> {
                var newline = false
                while (position < bufferEnd && buffer[position].let { it == ' ' || it == '\t' || it == '\r' || it == '\n' }) {
                    newline = newline || buffer[position] == '\n'
                    position++
                }
                type = T.WHITE_SPACE
                // A newline is a token in the compiler, so it ends a field access.
                if (newline) stateAfter = NORMAL
                finish(position, type, keepState = !newline)
                return
            }
            c == '/' && peek(position + 1) == '/' -> {
                while (position < bufferEnd && buffer[position] != '\n') position++
                finish(position, T.LINE_COMMENT, keepState = true)
                return
            }
            c == '"' -> {
                position = literalEnd(position + 1, '"')
                type = T.STRING
            }
            c == 'b' && (peek(position + 1) == '"' || peek(position + 1) == '\'') -> {
                val quote = buffer[position + 1]
                position = literalEnd(position + 2, quote)
                type = if (quote == '"') T.BYTE_STRING else T.BYTE_CHAR
            }
            isIdentifierStart(c) -> {
                while (position < bufferEnd && isIdentifierPart(buffer[position])) position++
                type = identifierType(buffer.subSequence(tokenStart, position).toString())
            }
            c in '0'..'9' && stateBefore == AFTER_DOT -> {
                while (position < bufferEnd && buffer[position] in '0'..'9') position++
                type = T.FIELD_INDEX
            }
            c in '0'..'9' -> {
                position = numberEnd(position)
                type = T.NUMBER
            }
            c == '@' && isIdentifierStart(peek(position + 1)) -> {
                position++
                while (position < bufferEnd && isIdentifierPart(buffer[position])) position++
                type = T.ATTRIBUTE
            }
            else -> {
                val symbol = SYMBOLS.firstOrNull { startsWith(position, it) }
                if (symbol != null) {
                    position += symbol.length
                    type = symbolType(symbol)
                } else {
                    position += Character.charCount(Character.codePointAt(buffer, position)).coerceAtMost(bufferEnd - position)
                    type = T.BAD_CHARACTER
                }
            }
        }
        stateAfter = if (type == T.DOT) AFTER_DOT else NORMAL
        finish(position, type, keepState = true)
    }

    private fun finish(end: Int, type: IElementType, keepState: Boolean) {
        tokenEnd = end
        tokenType = type
        if (!keepState) stateAfter = NORMAL
    }

    private fun peek(index: Int): Char = if (index < bufferEnd) buffer[index] else '\u0000'

    private fun startsWith(index: Int, text: String): Boolean {
        if (index + text.length > bufferEnd) return false
        for (i in text.indices) if (buffer[index + i] != text[i]) return false
        return true
    }

    /** Returns the offset after the closing quote, or the line end for an unterminated literal. */
    private fun literalEnd(contentStart: Int, quote: Char): Int {
        var position = contentStart
        while (position < bufferEnd) {
            val c = buffer[position]
            when {
                c == '\n' || c == '\r' -> return position
                c == '\\' && position + 1 < bufferEnd && buffer[position + 1] != '\n' && buffer[position + 1] != '\r' -> position += 2
                c == quote -> return position + 1
                else -> position++
            }
        }
        return position
    }

    /** Follows the compiler's number grammar; invalid suffixes stay in the token for diagnostics. */
    private fun numberEnd(start: Int): Int {
        var position = start
        var radix = 10
        if (buffer[position] == '0') {
            when (peek(position + 1)) {
                'x' -> radix = 16
                'o' -> radix = 8
                'b' -> radix = 2
            }
        }
        if (radix != 10) position += 2
        while (position < bufferEnd && (buffer[position] == '_' || Character.digit(buffer[position], radix) >= 0)) position++
        if (radix == 10 && peek(position) == '.' && peek(position + 1) in '0'..'9') {
            position++
            while (position < bufferEnd && (buffer[position] in '0'..'9' || buffer[position] == '_')) position++
        }
        if (radix == 10 && (peek(position) == 'e' || peek(position) == 'E')) {
            var exponent = position + 1
            if (peek(exponent) == '+' || peek(exponent) == '-') exponent++
            if (peek(exponent) in '0'..'9' || peek(exponent) == '_') {
                position = exponent
                while (position < bufferEnd && (buffer[position] in '0'..'9' || buffer[position] == '_')) position++
            }
        }
        while (position < bufferEnd && isIdentifierPart(buffer[position])) position++
        return position
    }

    companion object {
        const val NORMAL = 0
        const val AFTER_DOT = 1

        private val SYMBOLS = listOf(
            "<<=", ">>=", "..=", ":=", "->", "=>", "==", "!=", "<=", ">=",
            "&&", "||", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=",
            "|=", "^=", "::", "..", "{", "}", "(", ")", "[", "]", ",", ";",
            ":", ".", "+", "-", "*", "/", "%", "=", "<", ">", "!", "?",
            "&", "|", "^", "~", "@",
        )

        fun isIdentifierStart(c: Char): Boolean = c in 'a'..'z' || c in 'A'..'Z' || c == '_'

        fun isIdentifierPart(c: Char): Boolean = isIdentifierStart(c) || c in '0'..'9'

        private fun identifierType(text: String): IElementType = when (text) {
            in T.KEYWORDS -> T.KEYWORD
            in T.PRIMITIVE_TYPES -> T.PRIMITIVE_TYPE
            in T.BUILTIN_TYPES -> T.BUILTIN_TYPE
            in T.LITERAL_KEYWORDS -> T.LITERAL_KEYWORD
            "self" -> T.SELF
            in T.BUILTIN_FUNCTIONS -> T.BUILTIN_FUNCTION
            else -> T.IDENTIFIER
        }

        private fun symbolType(symbol: String): IElementType = when (symbol) {
            "{" -> T.LBRACE
            "}" -> T.RBRACE
            "[" -> T.LBRACKET
            "]" -> T.RBRACKET
            "(" -> T.LPAREN
            ")" -> T.RPAREN
            "," -> T.COMMA
            ";" -> T.SEMICOLON
            "." -> T.DOT
            ":", "::" -> T.COLON
            else -> T.OPERATOR
        }
    }
}
