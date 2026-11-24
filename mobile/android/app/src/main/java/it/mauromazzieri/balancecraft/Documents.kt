package it.mauromazzieri.balancecraft

import android.content.Context
import android.content.Intent
import android.net.Uri
import java.io.File
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

data class Today(val id: String, val name: String, val path: String, val due: String)

/** A line of the tree: an area (depth 0) or a task. */
data class Node(
    val id: String,
    val name: String,
    val depth: Int,
    val due: String = "",
    val notes: String = "",
    val weight: Int? = null,
    val onHold: String? = null,
)

data class DocumentState(
    val ready: Boolean = false,
    val open: Boolean = false,
    val busy: Boolean = false,
    val today: List<Today> = emptyList(),
    val tree: List<Node> = emptyList(),
    /** The shared document could not be reached: changes wait on the phone. */
    val offline: Boolean = false,
    val error: String? = null,
)

/**
 * The shared document and its mirror on this phone. The engine works on the mirror, where each
 * change stays in a replica; the mirror is then carried to the document. All work runs on the IO
 * dispatcher, one task at a time.
 */
object Documents {
    private const val PREFS = "balancecraft"
    private const val ROUNDS = 5
    private val lock = Mutex()
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val mutableState = MutableStateFlow(DocumentState())
    val state: StateFlow<DocumentState> = mutableState

    private lateinit var app: Context
    private var uri: Uri? = null
    private var id: String? = null

    fun start(context: Context) {
        if (::app.isInitialized) return
        app = context.applicationContext
        scope.launch {
            lock.withLock {
                uri = prefs().getString("document", null)?.let(Uri::parse)
                id = prefs().getString("id", null)
                if (uri != null && id != null) refresh()
                mutableState.value = mutableState.value.copy(ready = true, open = id != null)
            }
        }
    }

    /** Uses a document the person picked. */
    fun choose(picked: Uri) = task {
        val text = read(picked)
        val found = check(text) ?: throw CoreException("the file is not a BalanceCraft document")
        use(picked, found)
        refresh()
    }

    /** Puts a new document in a file the person made. */
    fun create(made: Uri) = task {
        if (read(made).isNotBlank()) throw CoreException("the file is not empty")
        val file = File(app.filesDir, "new.balancecraft")
        file.delete()
        Core.ask("new") { put("mirror", file.path); put("state", replicas().path) }
        val text = file.readText()
        file.delete()
        val found = check(text) ?: throw CoreException("the document could not be made")
        put(mirror(found), text)
        use(made, found)
        put(pending(), text)
        write(made, text)
        refresh()
    }

    private fun use(document: Uri, found: String) {
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        app.contentResolver.takePersistableUriPermission(document, flags)
        prefs().edit().putString("document", document.toString()).putString("id", found).apply()
        uri = document
        id = found
        mutableState.value = mutableState.value.copy(open = true, today = emptyList(), tree = emptyList())
    }

    /** Looks at the document again: other computers may have changed it. */
    fun reload() = task { if (id != null) refresh() }

    /** A change: a command of the engine. */
    fun change(cmd: String, fill: JSONObject.() -> Unit = {}) = task {
        round(JSONObject().put("cmd", cmd).apply(fill))
        show()
    }

    fun dismiss() {
        mutableState.value = mutableState.value.copy(error = null)
    }

    private fun task(body: () -> Unit) {
        scope.launch {
            lock.withLock {
                mutableState.value = mutableState.value.copy(busy = true)
                runCatching(body).onFailure { e ->
                    mutableState.value = mutableState.value.copy(error = e.message ?: e.toString())
                }
                mutableState.value = mutableState.value.copy(busy = false)
            }
        }
    }

    private fun refresh() {
        round(JSONObject().put("cmd", "today"))
        show()
    }

    /** Merges the document in the mirror, runs `command` there, and carries the result back. */
    private fun round(command: JSONObject) {
        val document = uri ?: return
        val mirror = mirror(id ?: return)
        var shared = runCatching { fetch(document) }
            .onFailure { if (it is CoreException) throw it }
            .getOrNull()
        if (shared != null) put(mirror, shared) else if (!mirror.exists()) throw IOException("the document could not be read")
        // Once here, the change is in the replica: what follows may fail and be tried again later.
        run(command)
        if (shared == null) {
            mutableState.value = mutableState.value.copy(offline = true)
            return
        }
        val carried = runCatching {
            for (i in 0 until ROUNDS) {
                val ours = mirror.readText()
                if (ours == shared) return@runCatching true
                val now = fetch(document)
                if (now != shared) {
                    // Changed elsewhere meanwhile: merge that first.
                    shared = now
                    put(mirror, now)
                    run(JSONObject().put("cmd", "today"))
                    continue
                }
                put(pending(), ours)
                write(document, ours)
                shared = ours
            }
            mirror.readText() == shared
        }.getOrDefault(false)
        mutableState.value = mutableState.value.copy(offline = !carried)
    }

    /** The shared document. A write of ours that stopped halfway is done again first. */
    private fun fetch(document: Uri): String {
        val text = read(document)
        val pending = pending()
        if (pending.exists()) {
            if (check(text) == null) {
                val ours = pending.readText()
                write(document, ours)
                return ours
            }
            pending.delete()
        }
        val found = check(text) ?: throw CoreException("the document is damaged")
        if (found != id) throw CoreException("the file holds another document; open it again")
        return text
    }

    private fun run(command: JSONObject): Any? = Core.ask("run") {
        put("mirror", mirror(id ?: "").path)
        put("state", replicas().path)
        put("command", command)
    }

    private fun show() {
        val today = (run(JSONObject().put("cmd", "today")) as JSONObject).getJSONArray("tasks")
        val areas = (run(JSONObject().put("cmd", "list")) as JSONObject).getJSONArray("areas")
        val tree = ArrayList<Node>()
        walk(areas, 0, tree)
        mutableState.value = mutableState.value.copy(
            today = objects(today).map { t ->
                val path = t.getJSONArray("path")
                Today(
                    t.getString("id"),
                    t.optString("name"),
                    (0 until path.length() - 1).joinToString(" › ") { path.getString(it) },
                    t.optString("due"),
                )
            },
            tree = tree,
        )
    }

    private fun walk(list: JSONArray, depth: Int, out: MutableList<Node>) {
        for (o in objects(list)) {
            out.add(
                Node(
                    o.getString("id"), o.optString("name"), depth, o.optString("due"), o.optString("notes"),
                    if (o.has("weight")) o.optInt("weight") else null,
                    if (o.has("on_hold")) o.optString("on_hold") else null,
                ),
            )
            o.optJSONArray("tasks")?.let { walk(it, depth + 1, out) }
        }
    }

    private fun objects(a: JSONArray) = (0 until a.length()).map { a.getJSONObject(it) }

    private fun check(text: String): String? =
        runCatching { (Core.ask("check") { put("text", text) } as JSONObject).getString("document") }.getOrNull()

    private fun read(document: Uri): String =
        app.contentResolver.openInputStream(document)?.use { it.readBytes().toString(Charsets.UTF_8) }
            ?: throw IOException("the document could not be opened")

    /** Replaces the document's content and reads it back; [pending] goes when it is there. */
    private fun write(document: Uri, text: String) {
        app.contentResolver.openOutputStream(document, "wt")?.use { it.write(text.toByteArray()) }
            ?: throw IOException("the document could not be written")
        if (read(document) != text) throw IOException("the document was not written whole")
        pending().delete()
    }

    /** Writes one of the app's own files at once. */
    private fun put(file: File, text: String) {
        val part = File(file.parentFile, "${file.name}.part")
        part.outputStream().use {
            it.write(text.toByteArray())
            it.fd.sync()
        }
        if (!part.renameTo(file)) throw IOException("the phone's copy could not be written")
    }

    private fun prefs() = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun replicas() = File(app.filesDir, "replicas").apply { mkdirs() }

    /** A folder for each document: what lies beside a document is searched for conflict copies. */
    private fun mirror(document: String) =
        File(File(app.filesDir, "mirrors/$document").apply { mkdirs() }, "document.balancecraft")

    private fun pending() = File(app.filesDir, "pending-${id}.txt")
}
