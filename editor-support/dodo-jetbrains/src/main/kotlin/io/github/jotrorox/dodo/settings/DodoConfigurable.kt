package io.github.jotrorox.dodo.settings

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.BoundSearchableConfigurable
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.DialogPanel
import com.intellij.ui.dsl.builder.AlignX
import com.intellij.ui.dsl.builder.bindItem
import com.intellij.ui.dsl.builder.bindSelected
import com.intellij.ui.dsl.builder.bindText
import com.intellij.ui.dsl.builder.panel
import com.intellij.ui.dsl.builder.toNullableProperty
import com.intellij.ui.dsl.listCellRenderer.textListCellRenderer
import io.github.jotrorox.dodo.lsp.DodoServers

/** Settings | Languages & Frameworks | Dodo. Applying a change restarts the server. */
class DodoConfigurable(private val project: Project) :
    BoundSearchableConfigurable("Dodo", "reference.settings.dodo", "io.github.jotrorox.dodo.settings") {

    override fun createPanel(): DialogPanel {
        val local = DodoLocalSettings.getInstance(project).state
        val shared = DodoSettings.getInstance(project).state
        return panel {
            group("Language Server") {
                row {
                    checkBox("Enable the Dodo language server")
                        .bindSelected(local::serverEnabled)
                        .comment("Highlighting, folding, and live templates work without the compiler.")
                }
                row("Server path:") {
                    textFieldWithBrowseButton(FileChooserDescriptorFactory.singleFile().withTitle("Dodo Compiler"), project)
                        .bindText({ local.serverPath ?: DodoLocalSettings.DEFAULT_SERVER_PATH }, { local.serverPath = it })
                        .align(AlignX.FILL)
                        .comment(
                            "Executable name on PATH or a path to the compiler. Relative paths, \${workspaceFolder}, " +
                                "and \$PROJECT_DIR\$ use the project directory. The plugin adds the <code>lsp</code> argument.",
                        )
                }
                row("Protocol trace:") {
                    comboBox(TraceLevel.entries, textListCellRenderer { it?.label })
                        .bindItem(local::trace.toNullableProperty())
                        .comment("Logs LSP messages in the Dodo Language Server tool window.")
                }
            }
            group("Compiler (Shared With the Project)") {
                row("Check mode:") {
                    comboBox(CheckMode.entries, textListCellRenderer { it?.label })
                        .bindItem(shared::checkMode.toNullableProperty())
                }
                row("Build target:") {
                    textField()
                        .bindText({ shared.buildTarget ?: "" }, { shared.buildTarget = it })
                        .align(AlignX.FILL)
                        .comment("Named target from dodo.toml for diagnostics and platform imports. Empty uses the project default.")
                }
                row("Manifest path:") {
                    textFieldWithBrowseButton(FileChooserDescriptorFactory.singleFile().withTitle("Dodo Manifest"), project)
                        .bindText({ shared.manifestPath ?: "" }, { shared.manifestPath = it })
                        .align(AlignX.FILL)
                        .comment("Absolute or relative to the project directory. Empty discovers dodo.toml there.")
                }
                row("Target triple:") {
                    textField()
                        .bindText({ shared.target ?: "" }, { shared.target = it })
                        .align(AlignX.FILL)
                        .comment("LLVM target triple, for example wasm32-unknown-unknown. Empty uses the manifest target, otherwise the host.")
                }
            }
        }
    }

    override fun apply() {
        super.apply()
        DodoServers.restart(project)
    }
}
