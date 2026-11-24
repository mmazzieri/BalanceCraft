package it.mauromazzieri.balancecraft

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.json.JSONObject

private val Green = Color(0xFF2E6B4E)
private val PaleGreen = Color(0xFFA9D3BD)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        Documents.start(this)
        setContent {
            val colors = if (isSystemInDarkTheme()) darkColorScheme(primary = PaleGreen) else lightColorScheme(primary = Green)
            MaterialTheme(colorScheme = colors) { Surface(Modifier.fillMaxSize()) { App() } }
        }
    }

    override fun onResume() {
        super.onResume()
        Documents.reload()
    }
}

/** What a dialog edits: a task, or a new one or a new area ([node] is null). */
private data class Form(val node: Node?, val parent: String?)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun App() {
    val state by Documents.state.collectAsStateWithLifecycle()
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { if (it != null) Documents.choose(it) }
    val make = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) {
        if (it != null) Documents.create(it)
    }
    val choose = { pick.launch(arrayOf("*/*")) }
    if (!state.ready) return
    var areas by rememberSaveable { mutableStateOf(false) }
    var form by remember { mutableStateOf<Form?>(null) }
    val snack = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val doneText = stringResource(R.string.completed)
    val undo = stringResource(R.string.undo)
    val complete: (String) -> Unit = { id ->
        Documents.change("complete") { put("id", id) }
        scope.launch {
            if (snack.showSnackbar(doneText, undo) == SnackbarResult.ActionPerformed) {
                Documents.change("uncomplete") { put("id", id) }
            }
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.app_name)) },
                actions = {
                    if (state.open) {
                        if (areas) {
                            IconButton(onClick = { form = Form(null, null) }) { Icon(Icons.Filled.Add, stringResource(R.string.new_area)) }
                        }
                        IconButton(onClick = { Documents.reload() }) { Icon(Icons.Filled.Refresh, stringResource(R.string.reload)) }
                        TextButton(onClick = choose) { Text(stringResource(R.string.open)) }
                    }
                },
            )
        },
        bottomBar = {
            if (state.open) {
                NavigationBar {
                    NavigationBarItem(!areas, { areas = false }, icon = {}, label = { Text(stringResource(R.string.today)) })
                    NavigationBarItem(areas, { areas = true }, icon = {}, label = { Text(stringResource(R.string.areas)) })
                }
            }
        },
        snackbarHost = { SnackbarHost(snack) },
    ) { inner ->
        Column(Modifier.padding(inner).fillMaxSize()) {
            if (state.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            state.error?.let {
                Row(Modifier.fillMaxWidth().padding(start = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(it, Modifier.weight(1f), color = MaterialTheme.colorScheme.error)
                    TextButton(onClick = Documents::dismiss) { Text(stringResource(R.string.close)) }
                }
            }
            if (state.offline) {
                Text(stringResource(R.string.offline), Modifier.padding(16.dp, 4.dp), style = MaterialTheme.typography.bodySmall)
            }
            when {
                !state.open -> Welcome(choose) { make.launch("Life.balancecraft") }
                areas -> Tree(state, edit = { form = it }, complete = complete)
                else -> TodayList(state, open = { id -> state.tree.firstOrNull { it.id == id }?.let { form = Form(it, null) } }, complete = complete)
            }
        }
    }
    form?.let { f -> FormDialog(f, close = { form = null }, addInside = { form = Form(null, it) }) }
}

@Composable
private fun Welcome(choose: () -> Unit, make: () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(stringResource(R.string.welcome), modifier = Modifier.padding(vertical = 16.dp))
        Button(onClick = choose) { Text(stringResource(R.string.open_document)) }
        TextButton(onClick = make) { Text(stringResource(R.string.new_document)) }
    }
}

@Composable
private fun TodayList(state: DocumentState, open: (String) -> Unit, complete: (String) -> Unit) {
    if (state.today.isEmpty() && !state.busy) Text(stringResource(R.string.nothing), Modifier.padding(24.dp))
    LazyColumn {
        items(state.today, key = { it.id }) { t ->
            Line(t.name, listOf(t.path, t.due.replace('T', ' ')).filter { it.isNotEmpty() }.joinToString(" · "), { open(t.id) }) { complete(t.id) }
        }
    }
}

@Composable
private fun Line(title: String, detail: String, open: () -> Unit, complete: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = open).padding(start = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
            Text(title, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (detail.isNotEmpty()) Text(detail, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        IconButton(onClick = complete) { Icon(Icons.Filled.Check, stringResource(R.string.complete)) }
    }
    HorizontalDivider()
}

@Composable
private fun Tree(state: DocumentState, edit: (Form) -> Unit, complete: (String) -> Unit) {
    val hold = stringResource(R.string.on_hold)
    LazyColumn {
        items(state.tree, key = { it.id }) { n ->
            if (n.depth == 0) {
                Row(Modifier.fillMaxWidth().padding(start = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(n.name, Modifier.weight(1f), fontWeight = FontWeight.Bold)
                    IconButton(onClick = { edit(Form(null, n.id)) }) { Icon(Icons.Filled.Add, stringResource(R.string.new_task)) }
                }
                HorizontalDivider()
            } else {
                val detail = listOfNotNull(n.due.replace('T', ' ').ifEmpty { null }, n.onHold?.let { "$hold $it".trim() }).joinToString(" · ")
                Column(Modifier.padding(start = (16 * (n.depth - 1)).dp)) {
                    Line(n.name, detail, { edit(Form(n, null)) }) { complete(n.id) }
                }
            }
        }
    }
}

@Composable
private fun FormDialog(form: Form, close: () -> Unit, addInside: (String) -> Unit) {
    val node = form.node
    val area = node == null && form.parent == null
    var name by remember(form) { mutableStateOf(node?.name ?: "") }
    var due by remember(form) { mutableStateOf(node?.due ?: "") }
    var notes by remember(form) { mutableStateOf(node?.notes ?: "") }
    var weight by remember(form) { mutableStateOf(node?.weight?.toString() ?: "") }
    var deleting by remember(form) { mutableStateOf(false) }
    val save = {
        when {
            area -> Documents.change("add_area") { put("name", name) }
            node == null -> Documents.change("add") {
                put("parent", form.parent)
                put("name", name)
                if (due.isNotBlank()) put("due", due)
                if (notes.isNotBlank()) put("notes", notes)
                weight.toIntOrNull()?.let { put("weight", it) }
            }
            // Only what was changed, so that another computer's edit of the rest is kept.
            else -> Documents.change("edit") {
                put("id", node.id)
                if (name != node.name) put("name", name)
                if (due != node.due) put("due", due.ifBlank { null } ?: JSONObject.NULL)
                if (notes != node.notes) put("notes", notes.ifBlank { null } ?: JSONObject.NULL)
                if (weight != (node.weight?.toString() ?: "")) put("weight", weight.toIntOrNull() ?: JSONObject.NULL)
            }
        }
        close()
    }
    AlertDialog(
        onDismissRequest = close,
        title = { Text(stringResource(if (area) R.string.new_area else if (node == null) R.string.new_task else R.string.task)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                OutlinedTextField(name, { name = it }, label = { Text(stringResource(R.string.name)) })
                if (!area) {
                    OutlinedTextField(due, { due = it }, label = { Text(stringResource(R.string.due)) }, singleLine = true)
                    OutlinedTextField(weight, { weight = it }, label = { Text(stringResource(R.string.weight)) }, singleLine = true)
                    OutlinedTextField(notes, { notes = it }, label = { Text(stringResource(R.string.notes)) })
                }
                if (node != null) {
                    TextButton(onClick = { addInside(node.id) }) { Text(stringResource(R.string.subtask)) }
                    if (node.onHold == null) {
                        TextButton(onClick = { Documents.change("hold") { put("id", node.id) }; close() }) { Text(stringResource(R.string.hold)) }
                    } else {
                        TextButton(onClick = { Documents.change("resume") { put("id", node.id) }; close() }) { Text(stringResource(R.string.resume)) }
                    }
                    TextButton(onClick = {
                        if (deleting) {
                            Documents.change("delete") { put("id", node.id) }
                            close()
                        } else {
                            deleting = true
                        }
                    }) { Text(stringResource(if (deleting) R.string.delete_sure else R.string.delete)) }
                }
            }
        },
        confirmButton = { TextButton(onClick = save, enabled = name.isNotBlank()) { Text(stringResource(R.string.save)) } },
        dismissButton = { TextButton(onClick = close) { Text(stringResource(R.string.close)) } },
    )
}
