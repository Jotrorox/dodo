package io.github.jotrorox.dodo.editor

import com.intellij.lang.ASTNode
import com.intellij.lang.folding.FoldingBuilderEx
import com.intellij.lang.folding.FoldingDescriptor
import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import com.intellij.psi.util.siblings
import io.github.jotrorox.dodo.lang.DodoTokenTypes as T

/**
 * Folds multi-line `{}`, `[]`, and `()` groups, runs of line comments, and
 * `// region` ... `// endregion` markers (VS Code's `#region` spelling also works).
 */
class DodoFoldingBuilder : FoldingBuilderEx(), DumbAware {
    override fun buildFoldRegions(root: PsiElement, document: Document, quick: Boolean): Array<FoldingDescriptor> {
        val descriptors = mutableListOf<FoldingDescriptor>()
        val brackets = ArrayDeque<ASTNode>()
        val regions = ArrayDeque<ASTNode>()
        var commentRun: Pair<ASTNode, ASTNode>? = null

        fun flushComments() {
            val (first, last) = commentRun ?: return
            commentRun = null
            if (first !== last) add(descriptors, document, first, TextRange(first.startOffset, last.textRange.endOffset), "//...")
        }

        val first = root.firstChild ?: return FoldingDescriptor.EMPTY_ARRAY
        for (element in first.siblings()) {
            val node = element.node
            when (element.elementType) {
                T.LBRACE, T.LBRACKET, T.LPAREN -> brackets.addLast(node)
                T.RBRACE, T.RBRACKET, T.RPAREN -> {
                    val open = brackets.removeLastOrNull()
                    if (open != null && T.closingFor(open.elementType) == node.elementType) {
                        val placeholder = open.text + "..." + node.text
                        add(descriptors, document, open, TextRange(open.startOffset, node.textRange.endOffset), placeholder)
                    } else {
                        // An unbalanced closer: recover by discarding the stack.
                        brackets.clear()
                    }
                }
                T.LINE_COMMENT -> {
                    val text = element.text
                    when {
                        REGION_START.containsMatchIn(text) -> {
                            flushComments()
                            regions.addLast(node)
                            continue
                        }
                        REGION_END.containsMatchIn(text) -> {
                            flushComments()
                            val start = regions.removeLastOrNull()
                            if (start != null) {
                                val label = REGION_START.replace(start.text, "").trim().ifEmpty { "..." }
                                add(descriptors, document, start, TextRange(start.startOffset, node.textRange.endOffset), label)
                            }
                            continue
                        }
                    }
                    val run = commentRun
                    commentRun = if (run == null) node to node else run.first to node
                    continue
                }
            }
            if (element.elementType == T.WHITE_SPACE && commentRun != null && element.text.count { it == '\n' } <= 1) continue
            flushComments()
        }
        flushComments()
        return descriptors.toTypedArray()
    }

    private fun add(descriptors: MutableList<FoldingDescriptor>, document: Document, node: ASTNode, range: TextRange, placeholder: String) {
        if (document.getLineNumber(range.startOffset) == document.getLineNumber(range.endOffset)) return
        descriptors += FoldingDescriptor(node, range, null, placeholder)
    }

    override fun getPlaceholderText(node: ASTNode): String = "..."

    override fun isCollapsedByDefault(node: ASTNode): Boolean = false

    companion object {
        private val REGION_START = Regex("""^//\s*#?region\b""")
        private val REGION_END = Regex("""^//\s*#?endregion\b""")
    }
}
