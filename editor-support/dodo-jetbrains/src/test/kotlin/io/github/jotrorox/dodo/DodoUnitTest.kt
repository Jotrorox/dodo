package io.github.jotrorox.dodo

import com.intellij.execution.ExecutionException
import io.github.jotrorox.dodo.lsp.DodoProtocolTracer
import io.github.jotrorox.dodo.lsp.DodoServerOutput
import io.github.jotrorox.dodo.lsp.DodoServerPath
import io.github.jotrorox.dodo.lsp.LspMessageFramer
import io.github.jotrorox.dodo.settings.TraceLevel
import junit.framework.TestCase
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.file.Files
import kotlin.io.path.absolutePathString
import kotlin.io.path.createDirectories
import kotlin.io.path.createFile

class DodoUnitTest : TestCase() {
    private val noPath: (String) -> File? = { null }

    fun testServerPathResolution() {
        val project = Files.createTempDirectory("dodo-project")
        val compiler = project.resolve("target/debug").createDirectories().resolve("dodo").createFile()
        val dir = project.absolutePathString()

        assertEquals(compiler, DodoServerPath.resolve("\${workspaceFolder}/target/debug/dodo", dir, noPath))
        assertEquals(compiler, DodoServerPath.resolve("\$PROJECT_DIR\$/target/debug/dodo", dir, noPath))
        assertEquals(compiler, DodoServerPath.resolve("target/debug/dodo", dir, noPath))
        assertEquals(compiler, DodoServerPath.resolve("  ./target/../target/debug/dodo ", dir, noPath))
        assertEquals(compiler, DodoServerPath.resolve(compiler.absolutePathString(), null, noPath))
        assertEquals(File("/opt/dodo/bin/dodo").toPath(), DodoServerPath.resolve("dodo", null) { File("/opt/dodo/bin/$it") })
        // Paths with spaces need no quoting.
        val spaced = project.resolve("with space").createDirectories().resolve("dodo").createFile()
        assertEquals(spaced, DodoServerPath.resolve("with space/dodo", dir, noPath))
    }

    fun testServerPathErrors() {
        fun message(configured: String, dir: String?): String {
            try {
                DodoServerPath.resolve(configured, dir, noPath)
            } catch (error: ExecutionException) {
                return error.message!!
            }
            fail("$configured resolved")
            error("unreachable")
        }
        assertTrue(message("", "/p").contains("Set the Dodo server path"))
        assertTrue(message("dodo", "/p").contains("not found on PATH"))
        assertTrue(message("bin/dodo", null).contains("requires a project directory"))
        assertTrue(message("\${workspaceFolder}/dodo", null).contains("no directory"))
        assertTrue(message("/definitely/missing/dodo", null).contains("not found at"))
    }

    private fun frame(json: String): ByteArray {
        val body = json.toByteArray()
        return "Content-Length: ${body.size}\r\n\r\n".toByteArray() + body
    }

    fun testFramerReassemblesSplitAndMultibyteMessages() {
        val messages = mutableListOf<String>()
        val framer = LspMessageFramer { messages += it }
        val stream = frame("""{"id":1,"result":"größe 🐦"}""") + frame("""{"method":"x"}""")
        for (chunk in stream.toList().chunked(3)) framer.feed(chunk.toByteArray())
        assertEquals(listOf("""{"id":1,"result":"größe 🐦"}""", """{"method":"x"}"""), messages)
    }

    fun testTracerFormatsBothDirections() {
        val lines = mutableListOf<Pair<String, DodoServerOutput.Kind>>()
        var level = TraceLevel.MESSAGES
        val tracer = DodoProtocolTracer({ level }) { text, kind -> lines += text to kind }
        val sent = ByteArrayOutputStream()
        tracer.wrapOutgoing(sent).write(frame("""{"jsonrpc":"2.0","id":3,"method":"textDocument/hover","params":{"a":1}}"""))
        tracer.incoming.feed(frame("""{"jsonrpc":"2.0","id":3,"result":null}"""))
        tracer.incoming.feed(frame("""{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":1,"message":"boom"}}"""))
        assertTrue(sent.size() > 0)
        assertEquals("Sending request 'textDocument/hover - (3)'.", lines[0].first)
        assertTrue(lines[1].first, lines[1].first.matches(Regex("""Received response 'textDocument/hover - \(3\)' in \d+ms\.""")))
        assertEquals("[Error] boom" to DodoServerOutput.Kind.ERROR, lines[2])
        assertEquals("Received notification 'window/logMessage'.", lines[3].first)

        lines.clear()
        level = TraceLevel.VERBOSE
        tracer.wrapOutgoing(sent).write(frame("""{"jsonrpc":"2.0","method":"initialized","params":{}}"""))
        assertEquals("Sending notification 'initialized'.\nParams: {}\n", lines.single().first)

        lines.clear()
        level = TraceLevel.OFF
        tracer.wrapOutgoing(sent).write(frame("""{"jsonrpc":"2.0","method":"initialized","params":{}}"""))
        tracer.incoming.feed(frame("""{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"ready"}}"""))
        assertEquals(listOf("[Info] ready" to DodoServerOutput.Kind.SERVER), lines)

        // A requested shutdown is noticed at any trace level, but text mentioning it is not.
        tracer.wrapOutgoing(sent).write(frame("""{"jsonrpc":"2.0","id":4,"method":"textDocument/hover","params":{"text":"shutdown"}}"""))
        assertFalse(tracer.shutdownRequested)
        tracer.wrapOutgoing(sent).write(frame("""{"jsonrpc":"2.0","id":5,"method":"shutdown"}"""))
        assertTrue(tracer.shutdownRequested)
    }
}
