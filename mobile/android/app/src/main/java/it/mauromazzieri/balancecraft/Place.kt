package it.mauromazzieri.balancecraft

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import java.io.FileNotFoundException
import java.io.IOException

/**
 * A document by its name in a folder the person chose. The folder, and not the file, because the
 * access to a file is lost when a sync app replaces it.
 */
class Place(private val resolver: ContentResolver, val tree: Uri, val name: String) {
    companion object {
        const val EXTENSION = ".balancecraft"

        private fun uri(tree: Uri, id: String): Uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)

        /** The files of the folder: name and document id. */
        private fun files(resolver: ContentResolver, tree: Uri): Map<String, String> {
            val list = DocumentsContract.buildChildDocumentsUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
            val columns = arrayOf(Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME)
            val cursor = resolver.query(list, columns, null, null, null) ?: throw IOException("the folder could not be listed")
            return cursor.use { c ->
                val out = HashMap<String, String>()
                while (c.moveToNext()) out[c.getString(1) ?: continue] = c.getString(0) ?: continue
                out
            }
        }

        /** The documents of the folder. */
        fun documents(resolver: ContentResolver, tree: Uri): List<String> =
            files(resolver, tree).keys.filter { it.endsWith(EXTENSION) }.sorted()

        /** Makes an empty file, which must not be there. */
        fun make(resolver: ContentResolver, tree: Uri, name: String): Place {
            if (name in files(resolver, tree)) throw IOException("$name is there already")
            val root = uri(tree, DocumentsContract.getTreeDocumentId(tree))
            val made = DocumentsContract.createDocument(resolver, root, "application/octet-stream", name)
                ?: throw IOException("$name could not be made")
            if (files(resolver, tree)[name] != DocumentsContract.getDocumentId(made)) {
                DocumentsContract.deleteDocument(resolver, made)
                throw IOException("$name could not be made with this name")
            }
            return Place(resolver, tree, name)
        }
    }

    private fun uri(): Uri = uri(tree, files(resolver, tree)[name] ?: throw FileNotFoundException("$name is not in the folder"))

    fun read(): String =
        resolver.openInputStream(uri())?.use { it.readBytes().toString(Charsets.UTF_8) }
            ?: throw IOException("the document could not be opened")

    /** Replaces the content and reads it back. */
    fun write(text: String) {
        resolver.openOutputStream(uri(), "wt")?.use { it.write(text.toByteArray()) }
            ?: throw IOException("the document could not be written")
        if (read() != text) throw IOException("the document was not written whole")
    }
}
