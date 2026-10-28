package io.github.jotrorox.dodo.lsp

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.markdown.utils.doc.DocMarkdownToHtmlConverter
import com.intellij.model.Pointer
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.platform.backend.documentation.DocumentationResult
import com.intellij.platform.backend.documentation.DocumentationTarget
import com.intellij.platform.backend.documentation.DocumentationTargetProvider
import com.intellij.platform.backend.presentation.TargetPresentation
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import io.github.jotrorox.dodo.lang.DodoIcons
import io.github.jotrorox.dodo.lang.DodoLanguage
import org.eclipse.lsp4j.DefinitionParams
import org.eclipse.lsp4j.HoverParams
import org.eclipse.lsp4j.MarkupKind
import org.eclipse.lsp4j.TextDocumentIdentifier

/** Go to Declaration from inside a bundled source, which the platform does not synchronize. */
class DodoStdlibGotoDeclarationHandler : GotoDeclarationHandler {
    override fun getGotoDeclarationTargets(sourceElement: PsiElement?, offset: Int, editor: Editor?): Array<PsiElement>? {
        val file = sourceElement?.containingFile ?: return null
        val source = file.virtualFile as? DodoStdlibFile ?: return null
        val project = file.project
        val server = DodoServers.client(project) ?: return null
        val document = DodoStdlibSources.document(source) ?: return null
        val params = DefinitionParams(TextDocumentIdentifier(source.uri), DodoStdlibSources.position(document, offset))
        val result = runCatching { server.sendRequestSync { it.textDocumentService.definition(params) } }.getOrNull() ?: return null
        val locations = if (result.isLeft) {
            result.left.map { it.uri to it.range.start }
        } else {
            result.right.map { it.targetUri to it.targetSelectionRange.start }
        }
        return locations.mapNotNull { (uri, start) ->
            val target = server.descriptor.findFileByUri(uri) ?: return@mapNotNull null
            val psi = PsiManager.getInstance(project).findFile(target) ?: return@mapNotNull null
            val targetDocument = DodoStdlibSources.document(target) ?: return@mapNotNull psi
            psi.findElementAt(DodoStdlibSources.offset(targetDocument, start)) ?: psi
        }.toTypedArray()
    }
}

/** Hover and Quick Documentation inside bundled sources. */
class DodoStdlibDocumentationTargetProvider : DocumentationTargetProvider {
    override fun documentationTargets(file: PsiFile, offset: Int): List<DocumentationTarget> {
        val source = file.virtualFile as? DodoStdlibFile ?: return emptyList()
        val server = DodoServers.client(file.project) ?: return emptyList()
        val document = DodoStdlibSources.document(source) ?: return emptyList()
        val params = HoverParams(TextDocumentIdentifier(source.uri), DodoStdlibSources.position(document, offset))
        val hover = runCatching { server.sendRequestSync { it.textDocumentService.hover(params) } }.getOrNull()
            ?: return emptyList()
        val contents = hover.contents
        val markdown = if (contents.isRight) {
            contents.right.let { if (it.kind == MarkupKind.PLAINTEXT) "```\n${it.value}\n```" else it.value }
        } else {
            contents.left.joinToString("\n\n") { part ->
                if (part.isLeft) part.left else "```${part.right.language}\n${part.right.value}\n```"
            }
        }
        if (markdown.isBlank()) return emptyList()
        return listOf(DodoHoverTarget(file.project, markdown))
    }
}

private class DodoHoverTarget(private val project: Project, private val markdown: String) : DocumentationTarget {
    override fun createPointer(): Pointer<out DocumentationTarget> = Pointer.hardPointer(this)

    override fun computePresentation(): TargetPresentation =
        TargetPresentation.builder("Dodo").icon(DodoIcons.FILE).presentation()

    override fun computeDocumentation(): DocumentationResult =
        DocumentationResult.documentation(DocMarkdownToHtmlConverter.convert(project, markdown, DodoLanguage))
}
