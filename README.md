# FeatherTex

Editor LaTeX desktop nativo, leggero e veloce scritto in **Rust** con **Iced 0.13**, progettato per ricercatori, tesisti e autori di articoli scientifici.

---

## Funzionalità principali

### Compilazione rapida & Anteprima PDF integrata
- **Precompilazione automatica del preambolo (`.fmt`)**: riduce i tempi di compilazione incrementale cachando il preambolo in formato `.fmt` nella cartella `.build/`.
- **Anteprima PDF nativa**: rendering ad alta risoluzione tramite **Poppler / Cairo** su Linux (con fallback automatico a `pdftocairo` / `pdftoppm` su Windows e su sistemi privi di `poppler-glib`).
- **Navigazione SyncTeX bidirezionale**: doppio click o salto diretto dal PDF alla riga corrispondente del codice sorgente e viceversa.
- **Log degli errori strutturato**: parsing dei messaggi di `pdflatex` / `lualatex` / `xelatex` con riepilogo errori/warning e click per saltare subito alla riga interessata.

### Ricette di compilazione & Bibliografia
- **7 ricette integrate**:
  - `pdfLaTeX`
  - `pdfLaTeX + BibTeX`
  - `pdfLaTeX + Biber`
  - `LuaLaTeX`
  - `XeLaTeX`
  - `latexmk (Auto)`
  - `Glossari + BibTeX`
- **Ricette personalizzate**: creabili e salvate in `~/.config/feathertex/recipes.txt`.
- **Supporto commenti magici**: riconosce automaticamente `% !TeX root`, `% !TeX program` e `% !BIB program`.
- **Rilevamento automatico bibliografia**: avvia o suggerisce BibTeX/Biber quando il documento contiene citazioni non ancora risolte, con opzione `--shell-escape` attivabile dall'interfaccia.
- **Preset e pacchetti per la ricerca**: preset `"Paper / Ricerca"` e categoria **Bibliografia e Pubblicazione** con inserimento rapido per `biblatex`, `natbib`, `csquotes`, `booktabs`, `subcaption`, `glossaries`, `minted` e `orcidlink`.

### Progetti multi-file, Struttura (Outline) & Ricerca
- **File Master/Root**: fissa il file principale del progetto (`Fissa Master`) per compilare e analizzare sempre l'intero documento da qualsiasi capitolo aperto.
- **Pannello `Struttura` (Outline) multi-file**: esplora ricorsivamente i file inclusi con `\input`, `\include` e `\subfile`, con filtri rapidi (`Tutto`, `Sez.`, `Teor.`, `Label`, `File`) per saltare istantaneamente tra sezioni, teoremi, etichette e file.
- **Trova e Sostituisci (`Ctrl+F` / `Ctrl+H`)**: supporto case-sensitive (`Aa`), motore **Regex** integrato (con gruppi di cattura `$1..$9`) e ricerca su tutta la cartella di progetto (`Cartella`).
- **Strumenti di editing avanzati**: vai alla riga (`Ctrl+G`), commenta/decommenta (`Ctrl+/` o `Ctrl+Shift+7`), indentazione/de-indentazione di blocco (`Tab` / `Shift+Tab`).

### Autocompletamento scientifico & Gestione Asset
- **Completamento intelligente di `\cite{...}`** (e `\parencite`, `\citet`, `\citep`): legge i file `.bib` del progetto mostrando chiave, Autore, Anno e Titolo.
- **Completamento di `\ref{...}`, `\eqref{...}`, `\cref{...}`**: scansiona tutti i `\label{...}` dell'intero progetto mostrando il contesto di sezione o teorema.
- **Pannello `Asset` & Drag and Drop figure**: trascinando un'immagine (`.pdf`, `.png`, `.jpg`, `.svg`) nella finestra viene copiata nella cartella `figures/`, vengono importati automaticamente i pacchetti necessari (`graphicx` / `svg`) se assenti e viene inserito il blocco `\begin{figure}...\end{figure}`.
- **Generatore rapido di tabelle**: crea tabelle `tabular` o `booktabs` personalizzabili in pochi click.
- **Importazione BibTeX da DOI o arXiv ID**: scarica i metadati bibliografici e accoda la voce BibTeX direttamente nel file `.bib` del progetto.

### Simboli & Riconoscimento a mano libera (`Match`)
- **Palette simboli**: catalogo organizzato per categoria con inserimento diretto nell'editor.
- **Lavagna `Match`**: disegna un simbolo matematico a mano libera per trovare e inserire il comando LaTeX corrispondente.

### Controllo qualità per pubblicazione
- **Conteggio parole reale (`texcount`)**: mostrato nella barra di stato, utile per rispettare i limiti di abstract, conferenze e riviste.
- **Linter tipografico (`chktex`)**: segnala in modo non bloccante nel pannello `Log → Linter` errori tipici (es. `~` mancante prima di `\cite`/`\ref`, virgolette errate, spazi prima della punteggiatura).
- **Controllo ortografico LaTeX-aware (`hunspell`)**: supporta dizionari `IT`, `EN` e `IT+EN` (`Log → Ortografia`), ignorando comandi LaTeX e blocchi matematici.
- **Supporto `latexdiff`**: genera con un click il PDF con le modifiche evidenziate rispetto al commit Git `HEAD` o a una versione precedente del file `.tex`.

### Temi & Persistenza impostazioni
- Tema **chiaro** e **scuro**.
- Salvataggio automatico di tema, dimensione font, auto-chiusura parentesi/ambienti, precompilazione live e lingua ortografica in `~/.config/feathertex/settings.txt`.

---

## Scorciatoie da tastiera

| Scorciatoia | Azione |
| :--- | :--- |
| `Ctrl+N` | Nuovo file |
| `Ctrl+O` | Apri file |
| `Ctrl+S` | Salva file |
| `Ctrl+B` | Compila progetto |
| `Ctrl+F` | Trova |
| `Ctrl+H` | Trova e Sostituisci |
| `Ctrl+G` | Vai alla riga |
| `Ctrl+/` / `Ctrl+Shift+7` | Commenta / Decommenta riga o selezione |
| `Tab` / `Shift+Tab` | Indenta / De-indenta blocco |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Aumenta / Riduci / Ripristina dimensione font |
| `Ctrl+Tab` | Cambia tab aperto |
| `Ctrl+W` | Chiudi tab corrente |
| `Esc` | Chiudi pannelli/popup di ricerca o completamento |

---

## Progetto Demo incluso

Il repository include un progetto scientifico multi-file pronto per essere testato nella cartella `demo/`:
- `demo/main.tex` — file master principale (con commenti magici e preambolo di esempio)
- `demo/chapters/intro.tex` — capitolo introduttivo
- `demo/chapters/results.tex` — risultati, teoremi e tabelle
- `demo/references.bib` — file bibliografico BibTeX

Per provarlo, avvia FeatherTex e apri `demo/main.tex`, quindi premi `Ctrl+B` (o il pulsante di compilazione) con la ricetta `pdfLaTeX + BibTeX`.

È inoltre presente nel repository il video dimostrativo `feathertex_demo.mp4` con una panoramica delle funzionalità principali.

---

## Installazione e Packaging

### Arch Linux
È disponibile il `PKGBUILD` dedicato in `packaging/arch/`:

```bash
cd packaging/arch
makepkg -si
```

Pacchetti consigliati su Arch Linux per sfruttare tutte le funzionalità (compilazione, bibliografie, rendering nativo e controlli qualità):

```bash
sudo pacman -S texlive-basic texlive-latexrecommended texlive-bibtexextra biber poppler-glib cairo hunspell hunspell-it hunspell-en_us
```

### Debian / Linux Mint / Ubuntu
Se è già disponibile il pacchetto `.deb` nella cartella `dist/`:

```bash
sudo apt install ./dist/feathertex_0.1.0_amd64.deb
```

Oppure per generare il `.deb` localmente su Debian / Mint / Ubuntu:

```bash
bash packaging/debian/build-deb.sh
```

> **Nota sulla compatibilità `glibc`**: un pacchetto `.deb` compilato su Arch Linux è collegato a una versione di `glibc` più recente rispetto a distribuzioni LTS come Debian Stable o Linux Mint. Per distribuire un `.deb` compatibile con sistemi Debian/Mint/Ubuntu meno recenti, esegui `packaging/debian/build-deb.sh` direttamente su una macchina Debian/Ubuntu/Mint oppure usa il workflow CI su `ubuntu-22.04`.

### Windows
Su Windows è disponibile lo script PowerShell in `packaging/windows/`:

```powershell
.\packaging\windows\build-windows.ps1
```

**Prerequisiti su Windows**:
- Toolchain **Rust** (`cargo`, `rustc`)
- Distribuzione LaTeX (**MiKTeX** o **TeX Live**) nel `PATH`
- **Poppler** (`pdftocairo` / `pdftoppm`) nel `PATH` per il rendering delle pagine PDF nell'anteprima

### Compilazione da sorgente
Clona il repository, esegui i test e compila la build release:

```bash
cargo test
cargo build --release --locked
```

L'eseguibile compilato si troverà in `target/release/feathertex`. Su Linux non è richiesta alcuna libreria di sviluppo Poppler/Cairo a compile-time: `libpoppler-glib` e `libcairo` vengono caricate dinamicamente a runtime se presenti (altrimenti viene usato `pdftocairo`/`pdftoppm` come fallback); su Windows utilizza direttamente il backend basato su `pdftocairo` / `pdftoppm`.

### Release automatiche (GitHub Actions)
Il workflow `.github/workflows/release.yml` viene attivato al push di un tag `v*` e genera automaticamente gli artefatti di rilascio:
- Pacchetto `.deb` compilato su `ubuntu-22.04`
- Archivio `.zip` per Windows compilato su `windows-latest`

---

## Struttura del repository

```text
TexLite/
├── src/
│   ├── main.rs            # Stato applicazione, eventi e loop Iced
│   ├── ui.rs              # Interfaccia utente, pannelli laterali, log ed editor
│   ├── theme.rs           # Temi chiaro/scuro e gestione font
│   ├── document.rs        # Buffer documento, ricerca, indentazione, autocompletamento
│   ├── syntax.rs          # Evidenziazione sintassi LaTeX
│   ├── compiler.rs        # Compilazione, cache .fmt, SyncTeX e parsing log
│   ├── poppler_ffi.rs     # Binding FFI Poppler/Cairo su Linux
│   ├── recipes.rs         # Ricette di compilazione e persistenza configurazioni
│   ├── project.rs         # Risoluzione master/root, outline multi-file, .bib/.label, DOI/arXiv
│   ├── quality.rs         # Integrazione texcount, chktex, hunspell e latexdiff
│   ├── settings.rs        # Salvataggio impostazioni in ~/.config/feathertex/
│   ├── recent.rs          # Cronologia file recenti
│   ├── matcher.rs         # Riconoscimento simboli disegnati a mano (Match)
│   └── symbols.rs         # Catalogo simboli matematici e preset/librerie LaTeX
├── demo/                  # Progetto demo multi-file (main.tex, chapters/, references.bib)
├── packaging/
│   ├── arch/PKGBUILD      # Pacchetto Arch Linux
│   ├── debian/build-deb.sh
│   ├── linux/             # Icona SVG e file .desktop
│   └── windows/build-windows.ps1
├── .github/workflows/
│   └── release.yml        # Build automatica .deb e Windows .zip su tag v*
├── build.rs               # Rilevamento condizionale Poppler/Cairo su Linux
└── Cargo.toml             # Dipendenze e metadati crate (feathertex v0.1.0)
```

---

## Licenza

Distribuito sotto licenza **GNU General Public License v3.0 (o successiva)** (`GPL-3.0-or-later`). Vedi il file [LICENSE](LICENSE) per il testo completo.

