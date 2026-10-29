package io.github.jotrorox.dodo.lsp

import org.eclipse.lsp4j.jsonrpc.services.JsonRequest
import org.eclipse.lsp4j.services.LanguageServer
import java.util.concurrent.CompletableFuture

data class DodoStdlibSourceParams(val uri: String)

/** `dodo lsp` extensions; see "Bundled sources in other clients" in the editor guide. */
interface DodoLanguageServer : LanguageServer {
    @JsonRequest("dodo/stdlibSource")
    fun stdlibSource(params: DodoStdlibSourceParams): CompletableFuture<String>
}
