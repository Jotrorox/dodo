package io.github.jotrorox.dodo.lang

import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.TokenSet

class DodoTokenType(debugName: String) : IElementType(debugName, DodoLanguage)

object DodoTokenTypes {
    @JvmField val LINE_COMMENT = DodoTokenType("LINE_COMMENT")
    @JvmField val STRING = DodoTokenType("STRING")
    @JvmField val BYTE_STRING = DodoTokenType("BYTE_STRING")
    @JvmField val BYTE_CHAR = DodoTokenType("BYTE_CHAR")
    @JvmField val NUMBER = DodoTokenType("NUMBER")
    /** A tuple element index after `.`, as in `pair.0.1`; never part of a float. */
    @JvmField val FIELD_INDEX = DodoTokenType("FIELD_INDEX")
    @JvmField val IDENTIFIER = DodoTokenType("IDENTIFIER")
    @JvmField val KEYWORD = DodoTokenType("KEYWORD")
    @JvmField val PRIMITIVE_TYPE = DodoTokenType("PRIMITIVE_TYPE")
    @JvmField val BUILTIN_TYPE = DodoTokenType("BUILTIN_TYPE")
    @JvmField val LITERAL_KEYWORD = DodoTokenType("LITERAL_KEYWORD")
    @JvmField val SELF = DodoTokenType("SELF")
    @JvmField val BUILTIN_FUNCTION = DodoTokenType("BUILTIN_FUNCTION")
    @JvmField val ATTRIBUTE = DodoTokenType("ATTRIBUTE")
    @JvmField val LBRACE = DodoTokenType("{")
    @JvmField val RBRACE = DodoTokenType("}")
    @JvmField val LBRACKET = DodoTokenType("[")
    @JvmField val RBRACKET = DodoTokenType("]")
    @JvmField val LPAREN = DodoTokenType("(")
    @JvmField val RPAREN = DodoTokenType(")")
    @JvmField val COMMA = DodoTokenType(",")
    @JvmField val SEMICOLON = DodoTokenType(";")
    @JvmField val DOT = DodoTokenType(".")
    @JvmField val COLON = DodoTokenType(":")
    @JvmField val OPERATOR = DodoTokenType("OPERATOR")

    // Escape layers inside string and byte literals, produced only for highlighting.
    @JvmField val VALID_ESCAPE = DodoTokenType("VALID_ESCAPE")
    @JvmField val INVALID_ESCAPE = DodoTokenType("INVALID_ESCAPE")

    @JvmField val WHITE_SPACE: IElementType = TokenType.WHITE_SPACE
    @JvmField val BAD_CHARACTER: IElementType = TokenType.BAD_CHARACTER

    @JvmField val COMMENTS = TokenSet.create(LINE_COMMENT)
    @JvmField val STRINGS = TokenSet.create(STRING, BYTE_STRING, BYTE_CHAR)
    @JvmField val WHITESPACES = TokenSet.create(WHITE_SPACE)
    @JvmField val OPEN_BRACKETS = TokenSet.create(LBRACE, LBRACKET, LPAREN)
    @JvmField val CLOSE_BRACKETS = TokenSet.create(RBRACE, RBRACKET, RPAREN)

    /** Keyword spellings, mirroring the VS Code grammar's keyword scopes. */
    val KEYWORDS = setOf(
        "return", "if", "else", "for", "in", "break", "continue", "match", "await",
        "pub", "unsafe", "async", "extern", "mut", "const", "static", "dyn",
        "fn", "struct", "enum", "interface", "type", "let",
        "package", "import", "from", "stores", "requires_plain",
        "as",
    )
    val PRIMITIVE_TYPES = setOf(
        "bool", "void", "str",
        "i8", "i16", "i32", "i64", "isize",
        "u8", "u16", "u32", "u64", "usize",
        "f32", "f64",
    )
    val BUILTIN_TYPES = setOf("Self", "Option", "Result")
    val LITERAL_KEYWORDS = setOf("true", "false", "none")
    val BUILTIN_FUNCTIONS = setOf("some", "ok", "err", "assert", "assert_eq", "assert_ne")

    fun closingFor(open: IElementType): IElementType? = when (open) {
        LBRACE -> RBRACE
        LBRACKET -> RBRACKET
        LPAREN -> RPAREN
        else -> null
    }
}
