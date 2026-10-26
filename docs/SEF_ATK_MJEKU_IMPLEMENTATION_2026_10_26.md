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
- **Rrezik kritik për kthimet:** `FiscalBridgeService.ReturnAsync` në POS
  përdor `Guid.NewGuid()` për kërkesat e kthimit. Pas humbjes së përgjigjes,
  një klikim i ri mund të krijojë një kthim tjetër nëse mbetet sasi.
  Kërkohet idempotencë e qëndrueshme për `coupon.return` dhe
  `coupon.cancel`, si dhe `action.lookup`, përpara përdorimit real.
  **Në TEST mos përsërit kthim/anulim pas timeout-it; kontrollo POS.**
- Teste reale me ATK TEST, printerin, kopjen, kthimin, anulimin, QR,
  ndërprerjen e energjisë/internetit, versionet e dy aplikacioneve.
- Gjurmueshmëri e plotë nen për nen për gjithë PDF-në, audit i sigurisë,
  nënshkrimi i binarëve dhe konfirmim i fushës së certifikimit nga ATK.


## Implementim shtesë i 26.10.2026 — kthim/anulim i sigurt dhe çmim i saktë

**Kodi u shkrua në tri degët e zhvillimit; nuk u ekzekutua build/test në Windows dhe nuk është miratim ATK.**

- POS: `coupon.return` / `coupon.cancel` kërkojnë `requestId` UUID të qëndrueshëm.
  Rezervimi `STARTED` bëhet në transaksion SQLite `IMMEDIATE` përpara motorit
  fiskal. Çelësi `ACTION|mjeku|requestId` është unik; hash-i i përmbajtjes bllokon
  përdorimin e UUID-së me kupon/sasi/arsye tjetër. Në rast të përgjigjes së humbur
  `action.lookup` lexon kuponin ekzistues pa lëshim të ri. Nëse hyrja nuk gjendet,
  `UNKNOWN` / `ACTION_UNCERTAIN` bllokojnë një veprim tjetër derisa të verifikohet.
- MJEKU: `sef_mjeku_v2.sql` ruan UUID, payload-in e pandryshueshëm, kontekstin,
  statuset dhe eventet append-only për çdo kthim/anulim. Kërkesa shkruhet para IPC.
  `IN_FLIGHT → UNKNOWN` pas timeout-it, pa retry. UI ka historikun dhe
  butonin vetëm-lexim **Verifiko në POS**.
- Çmimet: kontrata v1 u zgjerua me `unitPriceTenThousandths` (njësi 0.0001 EUR).
  POS përdor vlerën precize për ndërtimin e kuponit, kurse `unitPriceCents` ruhet
  për klientët e vjetër. Rust refuzon çmime me më shumë se katër shifra pas presjes
  dhe llogarit totalet në cent me aritmetikë të plotë; POS kontrollon përputhjen
  e rrumbullakimit. Për rastet e vjetra hash-i i shitjes mbetet v1.
- Privatësia: `sef_mjeku_v3.sql` shton përshkrimin fiskal të veçantë për secilin
  rresht të vizitës. Në detajet e vizitës shfaqet fusha **Përshkrimi në kupon**.
  `sef_build_sale` refuzon kuponin nëse përshkrimi mungon dhe nuk përdor
  automatikisht titullin/shënimet klinike. Përshkrimi ngrin pas fiskalizimit.
  Përdoruesi duhet të shkruajë vetëm emrin fiskal të shërbimit, pa të dhëna personale.

### Teste shtesë të domosdoshme para çdo instalimi

1. `dotnet test tests/Programeri.Pos.Tests --filter "FullyQualifiedName~MjekuBridgeTests"`
   — përfshin UUID të njëjtë, payload tjetër, ndërprerje para/pas ruajtjes së
   kuponit, `action.lookup` dhe çmimin 1.2345 EUR.
2. `cargo test --manifest-path src-tauri/Cargo.toml sef`
   — migrimet, append-only, rezervimin e një veprimi të pasigurt, çmimet 4 shifra.
3. `npm run typecheck && npm run build` në `mjeku-ui`.
4. Në Windows TEST: kthim/anulim me timeout të simuluar, restart POS, lookup,
   kontroll i vetëm një kuponi; sasi të pjesshme, konflikt UUID, ndryshim arsyeje,
   çmime 0.0001/1.2345, TVSH, printim, QR, ë/ç, internet/energji offline.
5. Provo në POS TEST identitetin fiskal të operatorit dhe pajtueshmërinë e
   përshkrimeve me ATK; mblidh prova të nënshkrimit dhe të radhës fiskale.
6. Merr konfirmim zyrtar për fushën e certifikimit dhe për përditësimin e
   integrimit përpara prodhimit.

**Bllokues që mbeten:** Windows build dhe testet reale ATK TEST/printer/QR;
audit i plotë nen për nen, operatori fiskal i verifikuar, paketimi i nënshkruar,
konfirmimi i certifikimit. Nuk janë provuar në këtë sesion.

## Faza të implementuara pas auditimit — pa X/Z

**Faza 1 — klasifikim fiskal dhe operator:**
- POS: `payloadSchemaVersion=2`, `operatorName` nga statusi i motorit SEF
  dhe krahasim në server me operatorin e konfiguruar. Artikujt kanë
  `categoryCode` SM/BM/PM, `unitCode` EA/HUR/DAY, `taxCode` A/C/D/E;
  serveri validon kategorinë, njësinë, kodin dhe normën e TVSH-së.
  Hash-i i idempotencës për versionin e ri përfshin klasifikimin dhe operatorin;
  kërkesat e vjetra të protokollit nuk ndryshojnë.
- MJEKU: `sef_mjeku_v4.sql`, API Rust `sef_item_fiscal_catalog_*`,
  fusha të reja te detajet e vizitës, dhe dërgim i versionit v2.
  Fiskalizimi bllokohet derisa përdoruesi të ruajë kategorinë/njësinë dhe
  POS të konfirmojë operatorin. Asnjë diagnozë/note klinike nuk dërgohet.

**Faza 2 — integriteti lokal:**
- `sef_mjeku_v5.sql` ruan SHA-256 zinxhir për ngjarjet e reja fiskale.
  Kontrolli i zinxhirit kryhet para çdo ndryshimi të statusit, dhe është
  ekspozuar një buton vetëm-lexim në faqen `Kuponët SEF`.
  Ngjarjet historike para migrimit mbeten jashtë këtij zinxhiri;
  aplikacioni e raporton këtë numër në vend që ta fshehë.

**Faza 3 — parapamja e kuponit:**
- `coupon.preview` merr PNG direkt nga `SefFiscalQueue.PreviewCopyAsync` në POS,
  vetëm për një kupon ekzistues të lëshuar nga MJEKU, me QR dhe shenjën KOPJE.
  UI shfaq këtë PNG, por NUK e printon WebView-in dhe NUK krijon kupon të ri.
  Kopja fizike vazhdon të përdorë `coupon.copy`.

**Teste të shkruara, ende të paekzekutuara në Windows:**
- `MjekuBridgeTests` (POS): operatori, taksa, kategori/njësi, hash i
  përmbajtjes, parapamja e kuponit ekzistues pa fiskalizim të dytë.
- Testet Rust: validimi i kontratës v2, integriteti i eventeve të reja
  dhe mbrojtja kundër shkrimit/fshirjes së tyre.

**Mbeten pa u mbyllur:** ekzekutimi i build-it dhe testeve reale,
VAT/ATK category/unit nga konfigurimi real, nënshkrimi Authenticode,
hardening i signer allow-list të procesit, rikuperimi nga restart/offline
me pajisje reale, eksport administrativ CSV/PDF i historikut, dërgimi
elektronik i kuponit (email), pajtueshmëria e plotë nen-për-nen dhe
konfirmimi zyrtar i certifikimit. Nuk po ndryshohen ose ridizajnohen
Raportet X/Z sipas kërkesës së përdoruesit.

**Kujdes:** Këto commit-e janë implementim kodor në branch-e zhvillimi,
jo provë që u ekzekutuan, jo certifikim ATK dhe jo lëshim PROD.

**Shtesë: eksport administrativ CSV.** `sef_audit_export_csv` pranon vetëm
`require_owner` (pronar ose mjek administrator), verifikon zinxhirin SHA-256,
dhe ruan në Desktop një CSV me `kind,source_id,event_type,status,error_code,occurred_at,sha256`.
Asnjë ID pacienti, diagnozë, notë klinike ose sekret ATK nuk eksportohet.
Ngjarjet para migrimit kanë kolonë SHA-256 bosh, jo hash të fabrikuar.
Eksporti PDF mbetet jashtë kësaj faze.
