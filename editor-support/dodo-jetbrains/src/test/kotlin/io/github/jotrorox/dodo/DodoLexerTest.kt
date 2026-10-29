package io.github.jotrorox.dodo

import com.intellij.psi.tree.IElementType
import io.github.jotrorox.dodo.lang.DodoHighlightingLexer
import io.github.jotrorox.dodo.lang.DodoLexer
import junit.framework.TestCase
import java.io.File
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

class DodoLexerTest : TestCase() {
    private fun tokens(text: String, lexer: com.intellij.lexer.Lexer = DodoLexer()): List<Pair<IElementType, String>> {
        lexer.start(text)
        val result = mutableListOf<Pair<IElementType, String>>()
        while (lexer.tokenType != null) {
            if (lexer.tokenType != T.WHITE_SPACE) result += lexer.tokenType!! to text.substring(lexer.tokenStart, lexer.tokenEnd)
            lexer.advance()
        }
        return result
    }

    private fun typeOf(text: String, token: String, lexer: com.intellij.lexer.Lexer = DodoLexer()): IElementType? =
        tokens(text, lexer).firstOrNull { it.second == token }?.first

    fun testKeywordCategories() {
        assertEquals(T.KEYWORD, typeOf("pub interface Sensor: Named {", "interface"))
        assertEquals(T.KEYWORD, typeOf("fn poll(sensor: &mut dyn Sensor) {", "dyn"))
        assertEquals(T.KEYWORD, typeOf("pub fn view(&self) -> &i32 from(self) {", "from"))
        assertEquals(T.SELF, typeOf("pub fn view(&self) -> &i32 from(self) {", "self"))
        assertEquals(T.PRIMITIVE_TYPE, typeOf("value := asm(\"mrs {0}, primask\", out(reg) u32)", "u32"))
        assertEquals(T.BUILTIN_TYPE, typeOf("identity::<Option<i32>>(some(42))", "Option"))
        assertEquals(T.BUILTIN_FUNCTION, typeOf("identity::<Option<i32>>(some(42))", "some"))
        assertEquals(T.BUILTIN_FUNCTION, typeOf("assert_eq(main(), 0)", "assert_eq"))
        assertEquals(T.LITERAL_KEYWORD, typeOf("x = none", "none"))
        assertEquals(T.KEYWORD, typeOf("x as u8", "as"))
    }

    fun testIdentifiersContainingKeywordsRemainWhole() {
        for (name in listOf("return_value", "gift", "somewhere", "u128", "format_f32")) {
            assertEquals(name, listOf(T.IDENTIFIER to name), tokens(name))
        }
    }

    fun testAttributes() {
        assertEquals(T.ATTRIBUTE, typeOf("@ignore(\"reason\")", "@ignore"))
        assertEquals(T.ATTRIBUTE, typeOf("@interrupt(TIMER_IRQ_0)", "@interrupt"))
        assertEquals(T.ATTRIBUTE, typeOf("@section(\".ramfunc\") @align(4)", "@align"))
    }

    fun testTupleIndicesAreNeverFloats() {
        assertEquals(
            listOf(T.IDENTIFIER to "total", T.OPERATOR to ":=", T.IDENTIFIER to "pair", T.DOT to ".", T.FIELD_INDEX to "0",
                T.DOT to ".", T.FIELD_INDEX to "1", T.OPERATOR to "+", T.NUMBER to "1.5"),
            tokens("total := pair.0.1 + 1.5"),
        )
        // A line break is a token in the compiler, so it ends the field access.
        assertEquals(T.NUMBER, tokens("x.\n1.5").last().first)
        assertEquals(T.FIELD_INDEX, tokens("x. 0").last().first)
        assertEquals(T.NUMBER, tokens("x. // note\n0").last().first)
    }

    fun testNumbersKeepTheirBoundaries() {
        for (number in listOf("0xffu8", "0o77", "0b1010u32", "1_000usize", "42i64", "1.25f32", "2e-3", "42f64", "1_000.25e+2f64")) {
            assertEquals(number, listOf(T.NUMBER to number), tokens(number))
        }
        assertEquals(
            listOf(T.NUMBER to "0", T.OPERATOR to "..", T.NUMBER to "10", T.NUMBER to "0", T.OPERATOR to "..=", T.NUMBER to "9"),
            tokens("0..10 0..=9"),
        )
        assertEquals(listOf(T.IDENTIFIER to "value", T.OPERATOR to "<<=", T.NUMBER to "2"), tokens("value <<= 2"))
    }

    fun testStringsAndByteLiterals() {
        assertEquals(listOf(T.STRING to "\"fn // text\""), tokens("\"fn // text\""))
        assertEquals(listOf(T.BYTE_STRING to "b\"bytes\""), tokens("b\"bytes\""))
        assertEquals(listOf(T.BYTE_CHAR to "b'X'"), tokens("b'X'"))
        assertEquals(listOf(T.BYTE_CHAR to "b'\\''"), tokens("b'\\''"))
        assertEquals(listOf(T.LINE_COMMENT to "// \"return 42\""), tokens("// \"return 42\""))
    }

    fun testUnterminatedLiteralsStopAtTheLineEnd() {
        for (source in listOf("\"unfinished", "b\"unfinished", "b'x", "// comment", "\"ends in escape\\")) {
            val result = tokens("$source\nfn main() {}")
            assertEquals(source, source, result.first().second)
            assertEquals(source, T.KEYWORD to "fn", result[1])
        }
    }

    fun testEscapes() {
        val highlighting = DodoHighlightingLexer()
        assertEquals(T.VALID_ESCAPE, typeOf("\"\\u{1f600}\"", "\\u{1f600}", highlighting))
        assertEquals(T.VALID_ESCAPE, typeOf("b\"\\xFF\\n\"", "\\xFF", highlighting))
        assertEquals(T.VALID_ESCAPE, typeOf("b\"\\xFF\\n\"", "\\n", highlighting))
        assertEquals(T.INVALID_ESCAPE, typeOf("b\"\\u{41}\"", "\\u", highlighting))
        assertEquals(T.INVALID_ESCAPE, typeOf("\"\\q\"", "\\q", highlighting))
        assertEquals(T.INVALID_ESCAPE, typeOf("\"\\u{1234567}\"", "\\u", highlighting))
    }

    fun testBadCharactersAreIsolated() {
        assertEquals(listOf(T.IDENTIFIER to "a", T.BAD_CHARACTER to "é", T.IDENTIFIER to "b"), tokens("aéb"))
        assertEquals(listOf(T.BAD_CHARACTER to "\uD83D\uDC26"), tokens("\uD83D\uDC26"))
    }

    /** Restarting at any state-0 token must reproduce the same tokens, as incremental highlighting does. */
    fun testRepositoryExamplesLexAndRestartConsistently() {
        val examples = File("../../examples").walkTopDown().filter { it.extension == "dodo" }.toList()
        assertTrue("examples found", examples.isNotEmpty())
        for (file in examples) {
            val text = file.readText()
            val lexer = DodoLexer()
            lexer.start(text)
            val all = mutableListOf<Triple<Int, IElementType, Int>>()
            val restartPoints = mutableListOf<Int>()
            while (lexer.tokenType != null) {
                if (lexer.state == 0) restartPoints += all.size
                all += Triple(lexer.tokenStart, lexer.tokenType!!, lexer.state)
                assertTrue("${file.name}: unterminated literal at ${lexer.tokenStart}",
                    lexer.tokenType !in T.STRINGS || text[lexer.tokenEnd - 1] in "\"'")
                lexer.advance()
            }
            assertEquals(text.length, lexer.tokenStart)
            for (index in restartPoints.filterIndexed { i, _ -> i % 37 == 0 }) {
                val restarted = DodoLexer()
                restarted.start(text, all[index].first, text.length, 0)
                var position = index
                while (restarted.tokenType != null) {
                    assertEquals("${file.name} restart at ${all[index].first}", all[position], Triple(restarted.tokenStart, restarted.tokenType!!, restarted.state))
                    position++
                    restarted.advance()
                }
                assertEquals(all.size, position)
            }
        }
    }
}
