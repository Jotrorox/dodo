package io.github.jotrorox.dodo.lsp

import com.intellij.execution.filters.TextConsoleBuilderFactory
import com.intellij.execution.ui.ConsoleView
import com.intellij.execution.ui.ConsoleViewContentType
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.ui.content.ContentFactory
import java.time.LocalTime
import java.time.format.DateTimeFormatter

/**
 * The "Dodo Language Server" tool window: startup messages, the server's stderr and
 * `window/logMessage` output, and optional protocol traces. Text logged before the
 * window opens is kept, up to a bounded size, and shown when it is created.
 */
@Service(Service.Level.PROJECT)
class DodoServerOutput(private val project: Project) : Disposable {
    enum class Kind(val contentType: ConsoleViewContentType) {
        INFO(ConsoleViewContentType.SYSTEM_OUTPUT),
        ERROR(ConsoleViewContentType.ERROR_OUTPUT),
        SERVER(ConsoleViewContentType.NORMAL_OUTPUT),
        TRACE(ConsoleViewContentType.LOG_DEBUG_OUTPUT),
    }

    private val lock = Any()
    private val history = ArrayDeque<Pair<String, Kind>>()
    private var historySize = 0
    private var console: ConsoleView? = null

    fun info(message: String) = log(message, Kind.INFO)

    fun error(message: String) = log(message, Kind.ERROR)

    fun log(message: String, kind: Kind) {
        val time = LocalTime.now().format(TIME)
        val text = message.lineSequence().joinToString("\n", postfix = "\n") { "[$time] $it" }
        val target = synchronized(lock) {
            history.addLast(text to kind)
            historySize += text.length
            while (historySize > MAX_HISTORY && history.size > 1) historySize -= history.removeFirst().first.length
            console
        }
        target?.print(text, kind.contentType)
        if (target == null) makeAvailable()
    }

    /** Lines logged so far; used by tests and by a newly opened console. */
    fun text(): String = synchronized(lock) { history.joinToString("") { it.first } }

    fun show() {
        ApplicationManager.getApplication().invokeLater({
            ToolWindowManager.getInstance(project).getToolWindow(TOOL_WINDOW_ID)?.let {
                it.isAvailable = true
                it.show()
            }
        }, project.disposed)
    }

    private fun makeAvailable() {
        ApplicationManager.getApplication().invokeLater({
            ToolWindowManager.getInstance(project).getToolWindow(TOOL_WINDOW_ID)?.isAvailable = true
        }, project.disposed)
    }

    internal fun attach(view: ConsoleView) {
        val entries = synchronized(lock) {
            console = view
            history.toList()
        }
        for ((text, kind) in entries) view.print(text, kind.contentType)
    }

    override fun dispose() {
        synchronized(lock) { console = null }
    }

    companion object {
        const val TOOL_WINDOW_ID = "Dodo Language Server"
        private const val MAX_HISTORY = 1 shl 20
        private val TIME = DateTimeFormatter.ofPattern("HH:mm:ss")

        fun getInstance(project: Project): DodoServerOutput = project.service()
    }
}

class DodoServerOutputToolWindowFactory : ToolWindowFactory, DumbAware {
    // Shown once the language server has logged something, or on request.
    override fun shouldBeAvailable(project: Project): Boolean = false

    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val console = TextConsoleBuilderFactory.getInstance().createBuilder(project).apply { setViewer(true) }.console
        val content = ContentFactory.getInstance().createContent(console.component, "", false)
        content.setDisposer(console)
        toolWindow.contentManager.addContent(content)
        val output = DodoServerOutput.getInstance(project)
        output.attach(console)
        Disposer.register(console) { output.dispose() }
    }
}
