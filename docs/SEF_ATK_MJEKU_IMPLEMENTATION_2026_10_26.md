# PROGRAMERI MJEKU → PROGRAMERI POS SEF: audit dhe implementim (26.10.2026)

**Status:** implementim i kodit në degë zhvillimi, jo miratim ATK dhe jo build i verifikuar në Windows.

## Burimet dhe kufiri i certifikimit

- Dokumenti zyrtar ATK, *Kërkesat Specifike Teknike dhe Funksionale për PEF/SF/SEF*,
  maj 2026, 88 faqe:
  https://apps.atk-ks.org/sefaplikimi/uploads/documents/150ab6cc-084f-490d-b5fc-d8639d0287dc.pdf
- Referenca e nxjerrë në repo POS:
  `docs/ATK Req/04_Kerkesat_Specifike_Teknike_Funksionale_PEF_SF_SEF.txt`.
  **OCR mund të ketë gabime; faqet/nënndarjet më poshtë duhen verifikuar vizualisht në PDF.**
- Kodi ekzistues POS: `src/Programeri.Pos.Fiscal/Bridge/{FiscalBridgeService.cs,SefBridgeEngine.cs,FiscalBridgeDispatcher.cs}`;
  `src/Programeri.Pos.Fiscal/Bridge/Windows/FiscalBridgePipeServer.cs`;
  `SefFiscalQueue`, `SefFiscalStore`, `SefSigner`, `SefAtkClient`.
- Kodi MJEKU: `src-tauri/src/sef_bridge.rs`, `src-tauri/src/lib.rs`,
  `src-tauri/src/db.rs`, `src-tauri/migrations/sef_mjeku_v1.sql`.
- POS mban nënshkrimin, identitetin fiskal, numërimin, ATK, ruajtjen fiskale,
  printimin dhe rregullat offline. MJEKU mban vetëm faturën komerciale,
  pasqyrën lokale të tentativave dhe lidhjen me kuponin e POS.
- **Certifikimi ekzistues i POS nuk vërteton automatikisht zgjerimin me MJEKU.**
  Konfirmimi i fushës së certifikimit duhet marrë nga ATK para prodhimit.

## Matricë gjurmueshmërie (pjesa SEF e dokumentit, jo provë e plotë certifikimi)

| ATK (faqe/nen) | Detyrimi | Rruga POS | Rruga MJEKU | Gjendja |
|---|---|---|---|---|
| f.43, n.24 | Regjistrim elektronik, nënshkrim dhe lidhje ATK | SefSigner, SefAtkClient, SefFiscalQueue | Nuk mban çelës/ATK; IPC | POS ekziston; E2E i paprovuar |
| f.43–44, n.25 | Instalimi dhe konfigurimi | PROGRAMERI POS + Build-SefWithMjeku.ps1 | mjeku_desktop.exe | Paketimi ekziston; release i ri i paprovuar |
| f.44, n.25 | Kupon për çdo transaksion dhe raportim | FiscalBridgeService.FiscalizeAsync → SefBridgeEngine | sef_fiscalize_blocking | Ekziston; testi ATK TEST mungon |
| f.44, n.25 | Struktura dhe nënshkrimi digjital | SefCouponBuilder/SefSigner | Nuk implementon fiskalizim të dytë | Përmes POS |
| f.44, n.25 | EUR, 4 shifra çmim njësie, 2 shifra total | SefCouponBuilder / POS domain | Bridge v1 ka unitPriceCents (2 shifra) | **GAP: 4 shifra për çmim njësie; analizë e kontratës/ATK e domosdoshme** |
| f.44–45, n.25 §§5.1–5.3 | ID unike, histori e pandryshueshme, audit | SefFiscalStore, FiscalBridgeCouponStore, security_audit | sef_mjeku_transactions/events, trigger-at immutable | Implementuar lokalisht; testet për t'u ekzekutuar |
| f.45, n.25 §6 | Ruajtje offline dhe ridërgim | SefFiscalQueue | PENDING_OFFLINE; nuk dërgon direkt ATK | Përmes POS; fault-test mungon |
| f.45, n.25 §7–8 | Miratim dhe përditësime | POS i certifikuar sipas pronarit | Ndryshimi i integrimit kërkon verifikim të fushës | **Bllokues për prodhim** |
| f.45, n.25 §9–11 | Kthim, ndërrim, anulim me referencë | FiscalBridgeService.ReturnAsync/CancelAsync | CouponsPage → coupon.return/cancel | Kthim/anulim ekzistojnë; ndërrimi end-to-end i paprovuar |
| f.45, n.25 §12 | TVSH A/C/D/E; pa qarkullim negativ | SefTaxCodeFor / FiscalBridgeValidator | Kodet A/C/D/E validim strikt | Validim i ri; test real i TVSH-së mungon |
| f.45–46, n.25 §13,15,17 | Raportim automatik, HTTPS, offline | SefAtkClient/SefFiscalQueue | Asnjë HTTP drejt ATK | Përmes POS; test me ATK TEST mungon |
| f.46, n.25 §14 | Shkronja latine/gjuhë zyrtare | POS receipt renderer | Titujt e shërbimeve nga MJEKU | Test ë/ç dhe printimi mungon |
| f.46–47, n.25 §18 | Kupon, preview, kopje, QR, elementet e detyrueshme | SefReceiptTextBuilder, SefCouponDelivery, PrintCopyAsync | coupon.copy, jo WebView print | Ekziston; printer/preview/QR i paprovuar |
| f.46–47, n.25 §18 | Operator, NUI, njësi, artikuj, sasi, çmime, TVSH, pagesa | POS wizard, SefCouponBuilder | Fiskalizimi dërgon shërbime/pagesa | **GAP: verifiko identitetin e operatorit dhe përshkrimet fiskale të shërbimeve** |

Pikat për pajisjet PEF/SF, karburantet dhe taksimetrat nuk u trajtuan si
detyrime të reja për modulin klinik. Kjo matricë nuk zëvendëson auditin e
plotë të 88 faqeve dhe shtojcave.

## Implementimi në këtë fazë

1. POS ka tashmë urë të autentikuar me Named Pipe, framing, PID/image-path
   verification dhe token seance; nuk u krijua një motor i ri SEF.
2. U shtua **`sale.lookup`**, operacion vetëm-lexim për identifikimin e
   kuponit sipas `(source=mjeku,saleId)`, edhe pas një ndërprerjeje.
   `NOT_FOUND` **nuk** autorizon automatikisht ridërgim.
3. MJEKU krijon snapshot komercial dhe SHA-256 para thirrjes, ruan
   `READY → IN_FLIGHT → FISCALIZED|PENDING_OFFLINE|UNKNOWN`.
   Çdo kalim ruhet si event append-only. Pas gabimit të transportit
   fiskalizimi **nuk** ridërgohet; kërkohet lookup.
4. UI `Kuponët` shfaq historikun lokal dhe butonin `Verifiko në POS`.
5. Fiskalizimi i ri refuzon pagesë të munguar/të pjesshme, kod TVSH të
   panjohur, sasi të pavlefshme dhe tejkalim të shumave. Nuk fabrikon
   pagesë cash ose ndryshim të metodës.
6. POS vazhdon të mbajë kuponët origjinalë dhe të dhënat e ATK;
   MJEKU nuk dërgon ID pacienti, diagnozë, ROM/MMT/NRS apo raporte.

## Skenarët e testimit në Windows (vetëm TEST)

| ID | Hapat | Rezultati i pritur |
|---|---|---|
| M-SEF-01 | Nis MJEKU jashtë POS dhe provo SEF | Refuzohet; asnjë kupon |
| M-SEF-02 | Nis nga POS, kontrollo status | TEST, konteksti dhe biznesi i saktë |
| M-SEF-03 | Shërbim 25€, pagesë cash 25€, TVSH e konfiguruar | Një kupon POS; MJEKU ruan ID/status |
| M-SEF-04 | Përsërit të njëjtin saleId | Nuk krijohet kupon i dytë |
| M-SEF-05 | Ndrysho çmimin/përmbajtjen pas tentativës | Bllokohet nga hash/snapshot |
| M-SEF-06 | Pagesë 10€ për faturë 25€ | Refuzohet para POS |
| M-SEF-07 | Pa pagesë ose TVSH e panjohur | Refuzohet para POS |
| M-SEF-08 | Ndërprit pipe-in pas dërgimit | UNKNOWN, pa retry automatik |
| M-SEF-09 | Rifillo POS, kliko Verifiko në POS | I njëjti kupon, asnjë emetim i ri |
| M-SEF-10 | Lookup NOT_FOUND ose UNKNOWN | Bllokim dhe verifikim manual |
| M-SEF-11 | Offline i lejuar | PENDING_OFFLINE, POS menaxhon radhën |
| M-SEF-12 | Kthe/anulo/kopjo kupon | Vetëm rruga ekzistuese POS |
| M-SEF-13 | Provo UPDATE/DELETE në sef_mjeku_events | Trigger-i SQLite refuzon |
| M-SEF-14 | Provo ë/ç, sasi me presje, TVSH, print preview/QR | Kontroll sipas kuponit real POS |
| M-SEF-15 | Fatura me titull që përmban diagnozë | **GAP: duhet projektuar përshkrimi i veçantë fiskal i shërbimit** |

**Nuk janë ekzekutuar këto teste në Windows në këtë sesion.** Testet e
reja Rust/xUnit janë shkruar, por mbeten për t'u kompiluar.

## Build i sigurt pa prekur checkout-in lokal të POS

Checkout-i lokal `pos-native-windows` ka ndryshime dhe histori të
diverguar. **Mos bëj reset/rebase/merge/stash automatik.** Përdor një
worktree të veçantë nga remote vetëm pasi të konfirmosh që nuk ka
përplasje me dosjet ekzistuese:

```powershell
cd "C:\Users\Admin\repos\pos-native-windows"
git fetch origin
git worktree add -b test/mjeku-sef-20261026 "..\pos-sef-mjeku-test" origin/agent/remove-invoice-llm

cd "..\pos-sef-mjeku-test"
dotnet test tests/Programeri.Pos.Tests --filter "FullyQualifiedName~MjekuBridgeTests"

cd "C:\Users\Admin\repos\Mjeku\mjeku-ui"
git pull --ff-only
npm run typecheck
npm run build

cd "C:\Users\Admin\repos\Mjeku\mjeku-desktop"
git pull --ff-only
cargo test --manifest-path src-tauri/Cargo.toml sef
```

Për paketimin e plotë përdor `scripts/Build-SefWithMjeku.ps1` nga
worktree i veçantë, por verifiko parametrat/path-et e tij para ekzekutimit;
skripti mund të presë vendndodhje specifike. Mos e konsidero paketën
TEST si certifikim të ri.

## Bllokues të hapur

- Çmimi për njësi me **4 shifra** (ATK f.44) kundrejt kontratës aktuale
  `unitPriceCents` me 2 shifra. Kërkon analizë të rrugës së certifikuar
  përpara ndryshimit të protokollit.
- Përshkrimi fiskal i shërbimit duhet të jetë i ndarë nga teksti klinik.
- Identiteti fiskal i operatorit duhet të verifikohet me ATK dhe kodin POS.
- Teste reale me ATK TEST, printerin, kopjen, kthimin, anulimin, QR,
  ndërprerjen e energjisë/internetit, versionet e dy aplikacioneve.
- Gjurmueshmëri e plotë nen për nen për gjithë PDF-në, audit i sigurisë,
  nënshkrimi i binarëve dhe konfirmim i fushës së certifikimit nga ATK.
