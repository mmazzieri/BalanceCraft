# BalanceCraft — piano di implementazione v1

Stato: v2, rivista dall'advisor il 23/11/2026 e corretta secondo le sue osservazioni (sezione finale). Decisioni di Mauro già prese (23/11/2026): nome BalanceCraft, repository pubblico `mmazzieri/balancecraft` (creato, vuoto), licenza MIT OR Apache-2.0, pubblicazione open source di modello dati, algoritmo e merge di Cerqua. Valutazione di partenza: Claude Doc «BalanceCraft: valutazione come Craft App».

## Obiettivo

App desktop locale in Rust che gestisce aree e attività con l'algoritmo di bilanciamento di Cerqua, e salva tutto in un unico file `.balancecraft` che l'utente può tenere in una cartella sincronizzata (OneDrive, Google Drive, Dropbox, Syncthing). Cerqua importerà quel file. Più avanti: candidatura come community app nell'ArtCraft Launcher.

Fuori dalla v1: account, server, AI, app mobile, build web, MCP/CLI, sincronizzazione diretta con API cloud.

## Vincoli

- Riscrittura pulita: nessun file TypeScript di Cerqua copiato. Si porta il comportamento, con il codice di Cerqua come specifica, e le prove di conformità (sotto).
- Licenze delle dipendenze solo dall'allowlist di `storytold/craft-contrib/standards/licences.md` (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, OFL per i font…), controllata con `cargo deny`. Niente GPL/LGPL/AGPL.
- Clean room verso Life Balance (Llamagraphics): nessun testo, icona o asset; il nome Life Balance non compare nell'interfaccia.
- `unsafe` vietato (`#![forbid(unsafe_code)]`), clippy `-D warnings`, `unwrap`/`expect`/`panic` negati nel codice di produzione: le stesse regole del launcher, così la candidatura non richiede rifacimenti.

## Architettura (workspace Cargo)

```
crates/
  model/     tipi serde: AppData, Area, TaskNode, HistoryEntry, AreaAction, GlobalSettings, EntityMeta
  engine/    algoritmo (quote, deficit, punteggio, lista di Oggi), operazioni dello store
             (add/edit/move/delete/complete/wait…), meta (touch, tombstone, reorder, cleanup), merge LWW
  file/      formato .balancecraft: busta, versioni, lettura/scrittura atomica, watcher,
             rilevamento e assorbimento delle copie di conflitto, lock locale
  ui-egui/   interfaccia egui
apps/
  balancecraft/   eseguibile desktop (eframe)
xtask/            ci, fixture, packaging
schema/           balancecraft-v1.schema.json
fixtures/         file di esempio e vettori di conformità generati da Cerqua
```

`model` ed `engine` sono puri (niente I/O, orologio passato come parametro) e compilano anche in wasm32, per una futura build web.

## Formato del file (v1)

```json
{
  "format": "balancecraft",
  "formatVersion": 1,
  "generator": "BalanceCraft 0.1.0",
  "documentId": "<uuid v4>",
  "savedAt": "2026-11-23T21:30:00Z",
  "data": { "areas": [], "history": [], "areaActions": [], "settings": {} },
  "extensions": {}
}
```

- `data` = `AppData` di Cerqua 1.46.0, `_meta` compreso, date dei task `YYYY-MM-DDTHH:MM` locali, `completedAt`/`at`/`createdAt` ISO. `_savedAt` non c'è: sta in `savedAt`.
- I tombstone restano nel file per 30 giorni (servono al merge tra copie); l'export verso Cerqua non serve, perché l'import di Cerqua li scarta (#131).
- Campi sconosciuti: conservati e riscritti (`#[serde(flatten)] extra: Map<String, Value>` su ogni entità e sulla busta), **e fusi**: il merge applica LWW a ogni chiave presente in `_meta.fields` di uno dei due lati, non solo alla lista nota dei campi tracciati. Un campo senza timbro e sconosciuto si tiene dal lato che lo ha; se lo hanno entrambi con valori diversi vince il disco. `extensions` si fonde per chiave di primo livello con la stessa regola. Così una 0.1 non annulla i campi nuovi scritti da una 0.2. Test: round-trip e merge di file con campi ignoti.
- Pareggi deterministici: a parità di timbro di campo vince il valore con JSON canonico minore (Cerqua dà ragione al locale: divergenza voluta, documentata nei vettori; da portare in Cerqua con una PR a parte, se Mauro vorrà).
- Timbri con orologio ibrido: `max(ora, timbro più alto visto + 1)`, compatibile con Cerqua. La scadenza dei tombstone ha un margine di 1 giorno.
- Deserializzazione tollerante come i backup reali di Cerqua: `children` assente o `null`, `_meta` assente, pesi decimali o assenti, `areaActions` e `settings` mancanti. Per i dati senza `_meta` si applica la logica di `migrateLegacyData`, con `savedAt` della busta al posto di `_savedAt`.
- `Salva come` genera un `documentId` nuovo.
- `formatVersion` maggiore di quello supportato: apertura rifiutata, in sola lettura nessuna scrittura.
- Id nuovi: 12 caratteri base36 casuali (Cerqua usa 7; verificato che nessun punto del codice di Cerqua ne assuma la lunghezza).
- JSON indentato e con chiavi in ordine stabile, per diff leggibili e per non far risincronizzare il file senza modifiche.

## Salvataggio su cartella sincronizzata

Modello di Cerqua: ogni PC ha una **replica locale** nella cartella dati dell'app (il «locale», come la cache `cerqua_data`), con il flag delle modifiche in sospeso e `lastMergedAt`. Il file condiviso è la «riga»: fa da remoto e da arbitro a parità di timbro di struttura.

1. Apertura: fonde sempre replica e disco (`merge(replica, disco)`). Se `lastMergedAt` è più vecchio di 30 giorni, prima scarta dalla replica le entità assenti dal disco e senza attività dopo quella data, perché i loro tombstone possono essere già stati purgati altrove.
2. Ogni modifica va subito nella replica (scrittura atomica locale); il file condiviso si scrive con una pausa di 1 s dopo l'ultima modifica e all'uscita.
3. Giro di scrittura: rilegge il disco, fonde, e scrive solo se il risultato è canonicamente diverso dal disco (come `canonical.ts`). Dopo qualche secondo rilegge il file: se non è quello scritto (il client di sync ha portato una versione nuova nella finestra), rifà il giro. Le modifiche non si perdono comunque, perché restano nella replica finché un giro non le ritrova sul disco.
4. Scrittura atomica: file temporaneo nella stessa cartella con un nome che i client ignorano (da verificare per OneDrive, Google Drive, Dropbox), `fsync`, poi su Windows `ReplaceFileW` (mantiene l'identità del file e la cronologia versioni di OneDrive), altrove `rename`. Nuovi tentativi con backoff su `ERROR_SHARING_VIOLATION` e `ACCESS_DENIED`, poi avviso non bloccante.
5. Cambi esterni: watcher `notify` più polling di riserva (stat e hash ogni 30–60 s), perché le unità virtuali di Google Drive e quelle di rete perdono eventi. Le proprie scritture si riconoscono dall'hash. L'mtime non è un segnale affidabile.
6. File su richiesta: la lettura gira fuori dal thread della UI con un timeout; se il segnaposto non si scarica (offline) l'app lo dice e lavora sulla replica.
7. Copie di conflitto: si cercano solo fra i `*.balancecraft*` della stessa cartella, escluso `.balancecraft-merged/`, senza aprire segnaposto non scaricati, e si confronta il `documentId`. Con un nome che segue gli schemi noti (OneDrive `nome-PC`, Google Drive `nome (1)`, Dropbox e Nextcloud `conflicted copy`, Syncthing `sync-conflict`) la copia si fonde da sola come «locale» contro il file principale, una alla volta in ordine di `savedAt` e poi di nome, e si sposta in `.balancecraft-merged/`. Con lo stesso id e un nome diverso è probabilmente una copia manuale: l'app chiede.
8. Lock locale (`fd-lock` sulla replica, nella cartella dati) contro due istanze sullo stesso documento sullo stesso PC.
9. Pulizia dei tombstone scaduti come Cerqua (30 giorni più il margine), con la salvaguardia per i discendenti vivi.
10. Backup locale delle ultime 10 versioni del file condiviso, prima di ogni sovrascrittura.

Rischio residuo accettato: un PC con l'orologio molto avanti vince gli LWW finché l'errore dura; l'orologio ibrido evita solo che i timbri tornino indietro.

## Conformità con Cerqua

Il punto debole di una riscrittura è la divergenza. Contromisura: vettori di prova generati dal codice di Cerqua.

- Uno script nel repository di Cerqua (`mobile/scripts/export-conformance.ts`, PR separata su Cerqua) esegue `computeFlatList`, `computeShares`, `mergeAppData`, `cleanupTombstones`, `reorderSiblings` sugli input dei test esistenti di Cerqua (`algorithm.test.ts`, `merge.test.ts`, `meta.test.ts`) e scrive coppie input/output JSON.
- I vettori si committano in `fixtures/conformance/` di BalanceCraft (dati sintetici, autore Mauro, MIT OR Apache-2.0) e i test Rust li ripercorrono tutti.
- Generatori deterministici: `uid`, `Date.now`, `TZ=Europe/Rome` e data di oggi bloccati; `_savedAt` tolto dagli output. Vettori anche con il cambio dell'ora legale e con date malformate. In Rust le date con `chrono`, e l'ordine delle operazioni sui float del punteggio uguale a quello di Cerqua.
- Prove di proprietà (`proptest`) sul merge (che per costruzione non è commutativo): punto fisso `merge(x, merge(x, D)) == merge(x, D)`; con scritture alternate di due repliche sullo stesso disco simulato si arriva a uno stato stabile in al più due giri; nessuna entità viva sparisce senza tombstone; nessun ciclo.
- Fixture dai backup reali di Cerqua (anonimizzati) per la deserializzazione tollerante.
- Il test di import su Cerqua (PR Cerqua) legge i file di `fixtures/files/` e controlla lo stato atteso.

## Import in Cerqua

`importData` di Cerqua è un ripristino: sostituisce tutto il cloud dell'account (#118, #131). Per un file BalanceCraft la proposta è un **import con merge**: Cerqua toglie la busta, controlla `formatVersion` e fonde `mergeAppData(locale, file)` senza ritimbrare, quindi nulla di quanto c'è solo in Cerqua viene cancellato (decisione di Mauro, sotto). La PR su Cerqua aggiunge anche: riconoscimento della busta in `pickAndImport`, MIME `application/vnd.balancecraft+json` e UTI su iOS e Android perché il selettore mostri i `.balancecraft`, messaggio chiaro se la versione del file è troppo nuova.

## Interfaccia (egui)

Quattro viste come i tab di Cerqua, in una finestra con barra laterale:
- **Oggi**: lista ordinata per punteggio, completa, metti in attesa, modifica, aggiungi sotto-attività; filtro per area; semaforo scadenze con preavviso in giorni lavorativi.
- **Albero**: aree e rami, aggiunta, modifica, spostamento (trascinamento e comando «Sposta in…»), riordino, cancellazione con conferma.
- **Aree**: stato, prossima azione, log delle azioni, silenzio dell'area.
- **In attesa** e **Storico**.
Più: impostazioni globali, File → Nuovo / Apri / Apri recenti / Salva come, indicatore di stato del file (salvato, in salvataggio, fuso con modifiche esterne, errore).
Lingue: italiano e inglese, stringhe in un catalogo (`fluent` o tabella come il launcher). Accessibilità: AccessKit di egui attivo, navigazione da tastiera completa.
Aree di default al primo «Nuovo documento»: quelle di Cerqua.

Le operazioni dello store stanno in `engine` come comandi puri, con nome e argomenti serializzabili: una CLI o un server MCP futuri (che dovranno seguire RFC-0015) li riusano senza rifacimenti.

## Identità dell'app (fissata prima della 0.1)

Il launcher blocca queste identità dopo l'ingresso nel catalogo, quindi si decidono ora: id `balancecraft`, binario `balancecraft`, bundle id e flatpak id `it.mauromazzieri.balancecraft`, `startup_wm_class` uguale, pacchetti deb/rpm `balancecraft`, publisher MSI «Mauro Mazzieri», UpgradeCode MSI generato una volta e mai cambiato, team macOS quello dell'account Apple di Mauro.

## Provenienza (clean room)

Il README e `ATTRIBUTION.md` dichiarano che modello, algoritmo e merge vengono da Cerqua, progetto di Mauro, disegnati in proprio a partire dall'idea pubblica di bilanciare le aree di vita, senza fonti interne di Llamagraphics. Il nome BalanceCraft si controlla con `craft-contrib/standards/naming.md`.

## Packaging e release (0.2)

- GitHub Actions: CI (fmt, clippy, test, `cargo deny`) su Linux, Windows, macOS.
- Release al tag: MSI (cargo-wix), DMG universale firmato e notarizzato (Developer ID di Mauro), AppImage, .deb, .rpm, tar.gz; `SHA256SUMS.txt`; nomi dei file come quelli attesi dal launcher.
- Associazione dell'estensione `.balancecraft` e del MIME `application/vnd.balancecraft+json`.
- Secret di firma Apple nei secret del repository: li inserisce Mauro.

## Fasi e consegne

1. **Scheletro**: workspace, licenze (LICENSE-MIT, LICENSE-APACHE, NOTICE), README, AGENTS.md, CI, `deny.toml` con `[graph] targets` limitato a Windows, macOS e Linux (per escludere dipendenze solo-UEFI come `r-efi`). Stringhe in una tabella come il launcher, non `fluent` (la sua dipendenza `self_cell` ha una licenza da verificare).
2. **model + file**: tipi tolleranti, busta, round-trip e merge dei campi ignoti, JSON Schema, test.
3. **engine**: algoritmo e meta con i vettori di Cerqua; poi merge (la parte più lunga: circa 630 righe di TS, 94 test).
4. **Sincronizzazione**: replica locale, giro di scrittura, verifica dopo la scrittura, watcher e polling, copie di conflitto, backup; test d'integrazione con cartella temporanea e scritture concorrenti simulate.
5. **UI 0.1**: Oggi e Albero, file, impostazioni.
6. **0.2**: Aree, In attesa, Storico; pacchetti per le tre piattaforme, firma macOS, associazione dell'estensione.
7. **Cerqua**: PR per lo script dei vettori e per l'import con merge (rilascio in produzione: serve il sì di Mauro al merge).
8. **Launcher**: issue/PR su `storytold/craft-launcher` per l'ingresso come community app.

Ogni fase è una PR verso `main` di `mmazzieri/balancecraft`, che Mauro unisce.

## Revisione dell'advisor (23/11/2026)

Accolte tutte le osservazioni: replica locale per PC (era il rischio principale, perdita silenziosa di modifiche), import in Cerqua come merge e non come ripristino, merge anche dei campi sconosciuti, pareggi deterministici, orologio ibrido, regola per le repliche ferme da più di 30 giorni, proprietà del merge corrette, vettori deterministici, deserializzazione tollerante, polling di riserva e `ReplaceFileW`, identità di installazione fissata subito, provenienza scritta, `deny.toml` per piattaforma, 0.1 ridotta a Oggi e Albero. Risposte alle domande aperte: il disco come arbitro va bene con la replica locale e i pareggi deterministici; le copie di conflitto si riconoscono per nome e `documentId` insieme; la v1 resta senza CLI/MCP ma con uno strato di comandi puro.
