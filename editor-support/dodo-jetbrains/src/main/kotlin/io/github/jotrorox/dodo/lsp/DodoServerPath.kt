package io.github.jotrorox.dodo.lsp

import com.intellij.execution.ExecutionException
import com.intellij.execution.configurations.PathEnvironmentVariableUtil
import com.intellij.openapi.util.SystemInfo
import java.io.File
import java.nio.file.Path
import kotlin.io.path.Path
import kotlin.io.path.isRegularFile

/**
 * Resolves the configured compiler like the VS Code extension: a bare name is
 * looked up on PATH, `${'$'}{workspaceFolder}` (or `${'$'}PROJECT_DIR${'$'}`) and relative
 * paths use the project directory, and absolute paths are used as given.
 */
object DodoServerPath {
    private val PROJECT_VARIABLES = listOf("\${workspaceFolder}", "\$PROJECT_DIR\$")

    @Throws(ExecutionException::class)
    fun resolve(
        configured: String,
        projectDir: String?,
        findInPath: (String) -> File? = ::findOnPath,
    ): Path {
        var path = configured.trim()
        if (path.isEmpty()) throw ExecutionException("Set the Dodo server path to the Dodo executable.")
        for (variable in PROJECT_VARIABLES) {
            if (path.contains(variable)) {
                projectDir ?: throw ExecutionException("The Dodo server path uses $variable, but the project has no directory.")
                path = path.replace(variable, projectDir)
            }
        }
        if (path == "~" || path.startsWith("~/")) path = System.getProperty("user.home") + path.substring(1)
        if (!path.contains('/') && !path.contains('\\')) {
            val found = findInPath(path)
                ?: throw ExecutionException("'$path' was not found on PATH. Install Dodo or set the Dodo server path to the compiler executable.")
            return found.toPath()
        }
        val resolved = Path(path).let { if (it.isAbsolute) it else resolveRelative(it, projectDir) }.normalize()
        if (!resolved.isRegularFile()) throw ExecutionException("The Dodo compiler was not found at $resolved.")
        return resolved
    }

    private fun resolveRelative(path: Path, projectDir: String?): Path {
        projectDir ?: throw ExecutionException("A relative Dodo server path requires a project directory. Use an absolute path or dodo on PATH.")
        return Path(projectDir).resolve(path)
    }

    private fun findOnPath(name: String): File? {
        if (SystemInfo.isWindows && !name.contains('.')) {
            PathEnvironmentVariableUtil.findInPath("$name.exe")?.let { return it }
        }
        return PathEnvironmentVariableUtil.findInPath(name)
    }
}
