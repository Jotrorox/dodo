package io.github.jotrorox.dodo.lsp

import com.intellij.ide.trustedProjects.TrustedProjectsListener
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.project.DumbAwareAction
import com.intellij.openapi.project.Project

class DodoRestartServerAction : DumbAwareAction() {
    override fun actionPerformed(event: AnActionEvent) {
        val project = event.project ?: return
        DodoServers.restart(project)
        if (!DodoServers.shouldRun(project)) {
            DodoServerOutput.getInstance(project).info("The language server is disabled or the project is not trusted.")
        }
    }

    override fun update(event: AnActionEvent) {
        event.presentation.isEnabled = event.project != null
    }

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT
}

class DodoShowOutputAction : DumbAwareAction() {
    override fun actionPerformed(event: AnActionEvent) {
        event.project?.let { DodoServerOutput.getInstance(it).show() }
    }

    override fun update(event: AnActionEvent) {
        event.presentation.isEnabled = event.project != null
    }

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT
}

/** The server runs only in trusted projects; start it once the user trusts the project. */
class DodoTrustListener : TrustedProjectsListener {
    override fun onProjectTrusted(project: Project) {
        DodoServers.restart(project)
    }
}
