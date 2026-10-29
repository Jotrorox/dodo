package io.github.jotrorox.dodo.lang

import com.intellij.lexer.LayeredLexer
import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors as Default
import com.intellij.openapi.editor.HighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.tree.IElementType
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

/** Color keys. Each falls back to a standard key, so every color scheme works unchanged. */
object DodoColors {
    @JvmField val KEYWORD = key("DODO_KEYWORD", Default.KEYWORD)
    @JvmField val PRIMITIVE_TYPE = key("DODO_PRIMITIVE_TYPE", Default.KEYWORD)
    @JvmField val BUILTIN_TYPE = key("DODO_BUILTIN_TYPE", Default.CLASS_REFERENCE)
    @JvmField val TYPE_NAME = key("DODO_TYPE_NAME", Default.CLASS_NAME)
    @JvmField val LITERAL_KEYWORD = key("DODO_LITERAL_KEYWORD", Default.KEYWORD)
    @JvmField val SELF = key("DODO_SELF", Default.KEYWORD)
    @JvmField val BUILTIN_FUNCTION = key("DODO_BUILTIN_FUNCTION", Default.PREDEFINED_SYMBOL)
    @JvmField val FUNCTION_DECLARATION = key("DODO_FUNCTION_DECLARATION", Default.FUNCTION_DECLARATION)
    @JvmField val FUNCTION_CALL = key("DODO_FUNCTION_CALL", Default.FUNCTION_CALL)
    @JvmField val CONSTANT = key("DODO_CONSTANT", Default.CONSTANT)
    @JvmField val PACKAGE = key("DODO_PACKAGE", Default.IDENTIFIER)
    @JvmField val IDENTIFIER = key("DODO_IDENTIFIER", Default.IDENTIFIER)
    @JvmField val ATTRIBUTE = key("DODO_ATTRIBUTE", Default.METADATA)
    @JvmField val NUMBER = key("DODO_NUMBER", Default.NUMBER)
    @JvmField val FIELD_INDEX = key("DODO_FIELD_INDEX", Default.INSTANCE_FIELD)
    @JvmField val STRING = key("DODO_STRING", Default.STRING)
    @JvmField val VALID_ESCAPE = key("DODO_VALID_ESCAPE", Default.VALID_STRING_ESCAPE)
    @JvmField val INVALID_ESCAPE = key("DODO_INVALID_ESCAPE", Default.INVALID_STRING_ESCAPE)
    @JvmField val LINE_COMMENT = key("DODO_LINE_COMMENT", Default.LINE_COMMENT)
    @JvmField val OPERATOR = key("DODO_OPERATOR", Default.OPERATION_SIGN)
    @JvmField val BRACES = key("DODO_BRACES", Default.BRACES)
    @JvmField val BRACKETS = key("DODO_BRACKETS", Default.BRACKETS)
    @JvmField val PARENTHESES = key("DODO_PARENTHESES", Default.PARENTHESES)
    @JvmField val COMMA = key("DODO_COMMA", Default.COMMA)
    @JvmField val SEMICOLON = key("DODO_SEMICOLON", Default.SEMICOLON)
    @JvmField val DOT = key("DODO_DOT", Default.DOT)
    @JvmField val COLON = key("DODO_COLON", Default.OPERATION_SIGN)
    @JvmField val BAD_CHARACTER = key("DODO_BAD_CHARACTER", HighlighterColors.BAD_CHARACTER)

    private fun key(name: String, fallback: TextAttributesKey) = createTextAttributesKey(name, fallback)
}

/** The parsing lexer plus an escape-sequence layer inside literals. */
class DodoHighlightingLexer : LayeredLexer(DodoLexer()) {
    init {
        for (literal in listOf(T.STRING, T.BYTE_STRING, T.BYTE_CHAR)) {
            registerSelfStoppingLayer(DodoEscapeLexer(literal), arrayOf(literal), IElementType.EMPTY_ARRAY)
        }
    }
}

class DodoSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = DodoHighlightingLexer()

    override fun getTokenHighlights(tokenType: IElementType?): Array<TextAttributesKey> =
        pack(ATTRIBUTES[tokenType])

    companion object {
        private val ATTRIBUTES: Map<IElementType, TextAttributesKey> = mapOf(
            T.KEYWORD to DodoColors.KEYWORD,
            T.PRIMITIVE_TYPE to DodoColors.PRIMITIVE_TYPE,
            T.BUILTIN_TYPE to DodoColors.BUILTIN_TYPE,
            T.LITERAL_KEYWORD to DodoColors.LITERAL_KEYWORD,
            T.SELF to DodoColors.SELF,
            T.BUILTIN_FUNCTION to DodoColors.BUILTIN_FUNCTION,
            T.IDENTIFIER to DodoColors.IDENTIFIER,
            T.ATTRIBUTE to DodoColors.ATTRIBUTE,
            T.NUMBER to DodoColors.NUMBER,
            T.FIELD_INDEX to DodoColors.FIELD_INDEX,
            T.STRING to DodoColors.STRING,
            T.BYTE_STRING to DodoColors.STRING,
            T.BYTE_CHAR to DodoColors.STRING,
            T.VALID_ESCAPE to DodoColors.VALID_ESCAPE,
            T.INVALID_ESCAPE to DodoColors.INVALID_ESCAPE,
            T.LINE_COMMENT to DodoColors.LINE_COMMENT,
            T.OPERATOR to DodoColors.OPERATOR,
            T.LBRACE to DodoColors.BRACES,
            T.RBRACE to DodoColors.BRACES,
            T.LBRACKET to DodoColors.BRACKETS,
            T.RBRACKET to DodoColors.BRACKETS,
            T.LPAREN to DodoColors.PARENTHESES,
            T.RPAREN to DodoColors.PARENTHESES,
            T.COMMA to DodoColors.COMMA,
            T.SEMICOLON to DodoColors.SEMICOLON,
            T.DOT to DodoColors.DOT,
            T.COLON to DodoColors.COLON,
            T.BAD_CHARACTER to DodoColors.BAD_CHARACTER,
        )
    }
}

class DodoSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        DodoSyntaxHighlighter()
}
