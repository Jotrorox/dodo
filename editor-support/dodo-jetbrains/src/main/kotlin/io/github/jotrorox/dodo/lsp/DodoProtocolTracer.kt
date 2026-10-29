package io.github.jotrorox.dodo.lsp

import com.google.gson.GsonBuilder
import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.github.jotrorox.dodo.settings.TraceLevel
import java.io.ByteArrayOutputStream
import java.io.FilterOutputStream
import java.io.OutputStream
import java.util.concurrent.ConcurrentHashMap

/**
 * Reassembles `Content-Length` framed JSON-RPC messages from a byte stream that
 * arrives in arbitrary chunks.
 */
class LspMessageFramer(private val onMessage: (String) -> Unit) {
    private val pending = ByteArrayOutputStream()

    @Synchronized
    fun feed(bytes: ByteArray, offset: Int = 0, length: Int = bytes.size) {
        pending.write(bytes, offset, length)
        var data = pending.toByteArray()
        var start = 0
        while (true) {
            val headerEnd = indexOf(data, HEADER_END, start)
            if (headerEnd < 0) break
            val headers = String(data, start, headerEnd - start, Charsets.US_ASCII)
            val length = headers.lineSequence()
                .map { it.trim() }
                .firstOrNull { it.startsWith("Content-Length:", ignoreCase = true) }
                ?.substringAfter(':')?.trim()?.toIntOrNull()
            val bodyStart = headerEnd + HEADER_END.size
            if (length == null) {
                // Not a frame header; drop it so one bad header cannot stall the trace.
                start = bodyStart
                continue
            }
            if (data.size - bodyStart < length) break
            onMessage(String(data, bodyStart, length, Charsets.UTF_8))
            start = bodyStart + length
        }
        if (start > 0) {
            data = data.copyOfRange(start, data.size)
            pending.reset()
            pending.write(data)
        }
    }

    private fun indexOf(data: ByteArray, pattern: ByteArray, from: Int): Int {
        outer@ for (i in from..data.size - pattern.size) {
            for (j in pattern.indices) if (data[i + j] != pattern[j]) continue@outer
            return i
        }
        return -1
    }

    private companion object {
        val HEADER_END = "\r\n\r\n".toByteArray(Charsets.US_ASCII)
    }
}

/**
 * Formats protocol messages in both directions like VS Code's LSP trace, and
 * forwards the server's `window/logMessage` notifications at any trace level.
 */
class DodoProtocolTracer(private val level: () -> TraceLevel, private val sink: (String, DodoServerOutput.Kind) -> Unit) {
    private val requestMethods = ConcurrentHashMap<String, Pair<String, Long>>()
    private val gson = GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create()

    /** Whether the client asked the server to shut down, so its termination is expected. */
    @Volatile
    var shutdownRequested = false
        private set

    val outgoing = LspMessageFramer { message(it, incoming = false) }
    val incoming = LspMessageFramer { message(it, incoming = true) }

    fun wrapOutgoing(stream: OutputStream): OutputStream = object : FilterOutputStream(stream) {
        override fun write(b: Int) {
            out.write(b)
            outgoing.feed(byteArrayOf(b.toByte()))
        }

        override fun write(b: ByteArray, off: Int, len: Int) {
            out.write(b, off, len)
            outgoing.feed(b, off, len)
        }
    }

    private fun message(body: String, incoming: Boolean) {
        if (!incoming && SHUTDOWN.containsMatchIn(body)) shutdownRequested = true
        val level = level()
        if (level == TraceLevel.OFF && !(incoming && body.contains(LOG_MESSAGE))) return
        val json = runCatching { JsonParser.parseString(body).asJsonObject }.getOrNull() ?: return
        val method = json.string("method")
        if (incoming && method == LOG_MESSAGE) {
            val params = json.getAsJsonObject("params")
            val type = params?.get("type")?.asInt ?: 4
            val text = params?.string("message") ?: ""
            sink("[${LOG_TYPES.getOrElse(type - 1) { "Log" }}] $text", if (type == 1) DodoServerOutput.Kind.ERROR else DodoServerOutput.Kind.SERVER)
            if (level == TraceLevel.OFF) return
        }
        if (level == TraceLevel.OFF) return
        val id = json.get("id")?.takeUnless { it.isJsonNull }?.toString()
        val direction = if (incoming) "Received" else "Sending"
        val (summary, details) = when {
            method != null && id != null -> {
                requestMethods[direction + id] = method to System.nanoTime()
                "$direction request '$method - ($id)'." to ("Params" to json.get("params"))
            }
            method != null -> "$direction notification '$method'." to ("Params" to json.get("params"))
            id != null -> {
                // A response answers a request sent in the opposite direction.
                val requestDirection = if (incoming) "Sending" else "Received"
                val (requestMethod, started) = requestMethods.remove(requestDirection + id) ?: ("unknown" to null)
                val elapsed = started?.let { " in ${(System.nanoTime() - it) / 1_000_000}ms" } ?: ""
                val error = json.get("error")
                val kind = if (error != null && !error.isJsonNull) "error response" else "response"
                "$direction $kind '$requestMethod - ($id)'$elapsed." to
                    (if (kind == "response") "Result" to json.get("result") else "Error" to error)
            }
            else -> return
        }
        val text = if (level == TraceLevel.VERBOSE) {
            val value: JsonElement? = details.second
            "$summary\n${details.first}: ${if (value == null) "No ${details.first.lowercase()}" else gson.toJson(value)}\n"
        } else {
            summary
        }
        sink(text, DodoServerOutput.Kind.TRACE)
    }

    private fun JsonObject.string(name: String): String? =
        get(name)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }?.asString

    private companion object {
        const val LOG_MESSAGE = "window/logMessage"
        val SHUTDOWN = Regex(""""method"\s*:\s*"(shutdown|exit)"""")
        val LOG_TYPES = listOf("Error", "Warning", "Info", "Log", "Debug")
    }
}
