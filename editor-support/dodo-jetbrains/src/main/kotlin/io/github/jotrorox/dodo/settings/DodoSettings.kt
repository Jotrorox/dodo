package io.github.jotrorox.dodo.settings

import com.intellij.openapi.components.BaseState
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.SimplePersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.components.StoragePathMacros
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project

enum class CheckMode(val value: String, val label: String) {
    FILE("file", "File: check each open file and its imports"),
    PACKAGE("package", "Package: check each open file's directory, including unsaved siblings"),
}

enum class TraceLevel(val label: String) {
    OFF("Off"),
    MESSAGES("Messages"),
    VERBOSE("Verbose"),
}

/**
 * Project compiler options shared through `.idea/dodo.xml`. They become the
 * server's initialization options, so changing them restarts the server.
 */
@Service(Service.Level.PROJECT)
@State(name = "DodoSettings", storages = [Storage("dodo.xml")])
class DodoSettings : SimplePersistentStateComponent<DodoSettings.Options>(Options()) {
    class Options : BaseState() {
        var checkMode by enum(CheckMode.FILE)
        var target by string("")
        var buildTarget by string("")
        var manifestPath by string("")
    }

    companion object {
        fun getInstance(project: Project): DodoSettings = project.service()
    }
}

/** Machine-specific options, kept in the workspace file and out of version control. */
@Service(Service.Level.PROJECT)
@State(name = "DodoLocalSettings", storages = [Storage(StoragePathMacros.WORKSPACE_FILE)])
class DodoLocalSettings : SimplePersistentStateComponent<DodoLocalSettings.Options>(Options()) {
    class Options : BaseState() {
        var serverEnabled by property(true)
        var serverPath by string(DEFAULT_SERVER_PATH)
        var trace by enum(TraceLevel.OFF)
    }

    companion object {
        const val DEFAULT_SERVER_PATH = "dodo"

        fun getInstance(project: Project): DodoLocalSettings = project.service()
    }
}
