package io.github.jotrorox.dodo.lang

import com.intellij.extapi.psi.PsiFileBase
import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.lexer.Lexer
import com.intellij.openapi.fileTypes.FileType
import com.intellij.openapi.project.Project
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet

class DodoFile(viewProvider: FileViewProvider) : PsiFileBase(viewProvider, DodoLanguage) {
    override fun getFileType(): FileType = DodoFileType

    override fun toString(): String = "Dodo file"
}

/**
 * A flat token tree. The compiler's language server owns all semantic analysis;
 * the IDE only needs tokens for highlighting, braces, comments, and folding.
 */
class DodoParserDefinition : ParserDefinition {
    override fun createLexer(project: Project?): Lexer = DodoLexer()

    override fun createParser(project: Project?): PsiParser = PsiParser { root, builder ->
        val marker = builder.mark()
        while (!builder.eof()) builder.advanceLexer()
        marker.done(root)
        builder.treeBuilt
    }

    override fun getFileNodeType(): IFileElementType = FILE

    override fun getCommentTokens(): TokenSet = DodoTokenTypes.COMMENTS

    override fun getStringLiteralElements(): TokenSet = DodoTokenTypes.STRINGS

    override fun getWhitespaceTokens(): TokenSet = DodoTokenTypes.WHITESPACES

    override fun createElement(node: ASTNode): PsiElement =
        throw UnsupportedOperationException("Dodo PSI has no composite elements: ${node.elementType}")

    override fun createFile(viewProvider: FileViewProvider): PsiFile = DodoFile(viewProvider)

    companion object {
        @JvmField
        val FILE = IFileElementType(DodoLanguage)
    }
}
