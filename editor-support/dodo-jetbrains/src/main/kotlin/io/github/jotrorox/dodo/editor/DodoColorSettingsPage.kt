package io.github.jotrorox.dodo.editor

import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import io.github.jotrorox.dodo.lang.DodoColors
import io.github.jotrorox.dodo.lang.DodoIcons
import io.github.jotrorox.dodo.lang.DodoSyntaxHighlighter
import javax.swing.Icon

class DodoColorSettingsPage : ColorSettingsPage {
    override fun getIcon(): Icon = DodoIcons.FILE

    override fun getHighlighter(): SyntaxHighlighter = DodoSyntaxHighlighter()

    override fun getDemoText(): String = DEMO

    override fun getAdditionalHighlightingTagToDescriptorMap(): Map<String, TextAttributesKey> = mapOf(
        "fn" to DodoColors.FUNCTION_DECLARATION,
        "call" to DodoColors.FUNCTION_CALL,
        "type" to DodoColors.TYPE_NAME,
        "const" to DodoColors.CONSTANT,
        "pkg" to DodoColors.PACKAGE,
    )

    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = DESCRIPTORS

    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY

    override fun getDisplayName(): String = "Dodo"

    companion object {
        private val DESCRIPTORS = arrayOf(
            AttributesDescriptor("Keywords//Keyword", DodoColors.KEYWORD),
            AttributesDescriptor("Keywords//Literal (true, false, none)", DodoColors.LITERAL_KEYWORD),
            AttributesDescriptor("Keywords//self", DodoColors.SELF),
            AttributesDescriptor("Types//Primitive type", DodoColors.PRIMITIVE_TYPE),
            AttributesDescriptor("Types//Built-in type (Self, Option, Result)", DodoColors.BUILTIN_TYPE),
            AttributesDescriptor("Types//Type name", DodoColors.TYPE_NAME),
            AttributesDescriptor("Functions//Declaration", DodoColors.FUNCTION_DECLARATION),
            AttributesDescriptor("Functions//Call", DodoColors.FUNCTION_CALL),
            AttributesDescriptor("Functions//Built-in (some, ok, err, assert)", DodoColors.BUILTIN_FUNCTION),
            AttributesDescriptor("Identifiers//Identifier", DodoColors.IDENTIFIER),
            AttributesDescriptor("Identifiers//Constant", DodoColors.CONSTANT),
            AttributesDescriptor("Identifiers//Package", DodoColors.PACKAGE),
            AttributesDescriptor("Identifiers//Tuple index", DodoColors.FIELD_INDEX),
            AttributesDescriptor("Attribute", DodoColors.ATTRIBUTE),
            AttributesDescriptor("Literals//Number", DodoColors.NUMBER),
            AttributesDescriptor("Literals//String and byte literal", DodoColors.STRING),
            AttributesDescriptor("Literals//Valid escape sequence", DodoColors.VALID_ESCAPE),
            AttributesDescriptor("Literals//Invalid escape sequence", DodoColors.INVALID_ESCAPE),
            AttributesDescriptor("Comment", DodoColors.LINE_COMMENT),
            AttributesDescriptor("Punctuation//Operator", DodoColors.OPERATOR),
            AttributesDescriptor("Punctuation//Braces", DodoColors.BRACES),
            AttributesDescriptor("Punctuation//Brackets", DodoColors.BRACKETS),
            AttributesDescriptor("Punctuation//Parentheses", DodoColors.PARENTHESES),
            AttributesDescriptor("Punctuation//Comma", DodoColors.COMMA),
            AttributesDescriptor("Punctuation//Semicolon", DodoColors.SEMICOLON),
            AttributesDescriptor("Punctuation//Dot", DodoColors.DOT),
            AttributesDescriptor("Punctuation//Colon", DodoColors.COLON),
            AttributesDescriptor("Bad character", DodoColors.BAD_CHARACTER),
        )

        private val DEMO = """
            package <pkg>main</pkg>

            import "std/console"

            const <const>LIMIT</const>: usize = 0x10

            // A point in the plane.
            struct <type>Point</type> {
                x: f64,
                y: f64,
            }

            interface <type>Shape</type> {
                fn <fn>area</fn>(&self) -> f64
            }

            @test
            fn <fn>main</fn>() -> i32 {
                let origin = <type>Point</type> { x: 0.0, y: 1.5e3f64 }
                count := 0i32
                let pair = (b'a', b"bytes\n")
                let first = pair.0
                match <call>lookup</call>(LIMIT) {
                    some(value) => console.<call>printf</call>("{}\u{1F426}\n", value)!,
                    none => assert(false),
                }
                if count != 0 && self_check() { return 1 }
                let bad = "\q"
                return 0
            }
        """.trimIndent()
    }
}
