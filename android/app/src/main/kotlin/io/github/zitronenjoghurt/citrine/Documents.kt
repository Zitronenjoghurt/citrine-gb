package io.github.zitronenjoghurt.citrine

import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.result.contract.ActivityResultContracts
import androidx.documentfile.provider.DocumentFile
import java.io.File

class Documents(private val activity: MainActivity) {
    private var openId = 0
    private var treeId = 0
    private var saveId = 0
    private val saving = ArrayDeque<Pair<String, ByteArray>>()
    private val saved = mutableListOf<String>()
    private var current: ByteArray? = null

    private val openOne = activity.registerForActivityResult(ActivityResultContracts.OpenDocument()) {
        read(openId, listOfNotNull(it))
    }

    private val openMany = activity.registerForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) {
        read(openId, it)
    }

    private val create = activity.registerForActivityResult(
        ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { created(it) }

    private val tree = activity.registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) {
        picked(it)
    }

    fun open(id: Int, multiple: Boolean) {
        openId = id
        val types = arrayOf("*/*")
        if (multiple) openMany.launch(types) else openOne.launch(types)
    }

    fun save(id: Int, files: List<Pair<String, ByteArray>>) {
        saveId = id
        saving.clear()
        saving.addAll(files)
        saved.clear()
        next()
    }

    fun openTree(id: Int) {
        treeId = id
        tree.launch(null)
    }

    fun listRoms(id: Int, tree: String) {
        Thread {
            val root = runCatching { DocumentFile.fromTreeUri(activity, Uri.parse(tree)) }.getOrNull()
            if (root == null || !root.canRead()) {
                activity.nativeRoms(id, emptyArray(), emptyArray(), false)
                return@Thread
            }
            val found = mutableListOf<Pair<String, String>>()
            runCatching { collectRoms(root, ROM_FOLDER_DEPTH, found) }
            found.sortBy { it.first.lowercase() }
            activity.nativeRoms(
                id,
                found.map { it.first }.toTypedArray(),
                found.map { it.second }.toTypedArray(),
                true,
            )
        }.start()
    }

    fun readRom(id: Int, uri: String) {
        read(id, listOf(Uri.parse(uri)))
    }

    fun copyIntoTree(tree: String, dir: File) {
        val root = DocumentFile.fromTreeUri(activity, Uri.parse(tree)) ?: return
        runCatching { copy(dir, root) }
    }

    private fun read(id: Int, uris: List<Uri>) {
        Thread {
            val names = mutableListOf<String>()
            val data = mutableListOf<ByteArray>()
            for (uri in uris) {
                val bytes = runCatching {
                    activity.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                }.getOrNull() ?: continue
                names += name(uri)
                data += bytes
            }
            activity.nativeOpened(id, names.toTypedArray(), data.toTypedArray())
        }.start()
    }

    private fun next() {
        val file = saving.removeFirstOrNull()
        if (file == null) {
            activity.nativeSaved(saveId, saved.toTypedArray(), null)
            return
        }
        current = file.second
        create.launch(file.first)
    }

    private fun created(uri: Uri?) {
        val data = current
        current = null
        if (uri == null || data == null) {
            saving.clear()
            activity.nativeSaved(saveId, saved.toTypedArray(), null)
            return
        }
        val error = runCatching {
            activity.contentResolver.openOutputStream(uri, "wt")?.use { it.write(data) }
                ?: error("the file could not be opened")
        }.exceptionOrNull()
        if (error != null) {
            saving.clear()
            activity.nativeSaved(saveId, saved.toTypedArray(), error.message ?: error.toString())
            return
        }
        saved += name(uri)
        next()
    }

    private fun picked(uri: Uri?) {
        if (uri != null) {
            runCatching {
                activity.contentResolver.takePersistableUriPermission(
                    uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
                )
            }
        }
        activity.nativeTree(treeId, uri?.toString())
    }

    private fun collectRoms(dir: DocumentFile, depth: Int, found: MutableList<Pair<String, String>>) {
        for (file in dir.listFiles()) {
            val name = file.name ?: continue
            if (file.isDirectory) {
                if (depth > 0) collectRoms(file, depth - 1, found)
            } else if (isRom(name)) {
                found += name to file.uri.toString()
            }
        }
    }

    private fun copy(from: File, into: DocumentFile) {
        for (file in from.listFiles().orEmpty()) {
            if (file.isDirectory) {
                val dir = into.findFile(file.name) ?: into.createDirectory(file.name) ?: continue
                copy(file, dir)
            } else {
                into.findFile(file.name)?.delete()
                val target = into.createFile("application/octet-stream", file.name) ?: continue
                activity.contentResolver.openOutputStream(target.uri, "wt")?.use { out ->
                    file.inputStream().use { it.copyTo(out) }
                }
            }
        }
    }

    private fun name(uri: Uri): String = runCatching {
        activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
        }
    }.getOrNull() ?: uri.lastPathSegment?.substringAfterLast('/') ?: "file"

    companion object {
        const val ROM_FOLDER_DEPTH = 2

        fun isRom(name: String): Boolean {
            val lower = name.lowercase()
            return lower.endsWith(".gb") || lower.endsWith(".gbc")
        }
    }
}
