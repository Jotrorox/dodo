package io.github.jotrorox.dodo.lsp

import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.options.ShowSettingsUtil
import com.intellij.openapi.project.Project
import io.github.jotrorox.dodo.settings.DodoConfigurable

object DodoNotifications {
    fun serverFailed(project: Project, message: String) {
        NotificationGroupManager.getInstance()
            .getNotificationGroup("Dodo")
            .createNotification(
                "Dodo language server could not start",
                "Install Dodo or check the Dodo server path. $message",
                NotificationType.ERROR,
            )
            .addAction(NotificationAction.createSimple("Open Settings") {
                ShowSettingsUtil.getInstance().showSettingsDialog(project, DodoConfigurable::class.java)
            })
            .addAction(NotificationAction.createSimple("Show Output") {
                DodoServerOutput.getInstance(project).show()
            })
            .notify(project)
    }
}
