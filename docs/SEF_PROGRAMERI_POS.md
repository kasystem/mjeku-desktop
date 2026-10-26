# SEF (PROGRAMERI POS) te Mjeku

Statusi: kodi është shkruar (Faza 2 dhe 3 të planit); **nuk është ndërtuar si aplikacion i plotë dhe nuk është provuar me PROGRAMERI POS**. Shih "Çfarë është provuar".

**Përditësim 26.10.2026:** U shtuan regjistri lokal SEF i pandryshueshëm, `sale.lookup` dhe rikuperimi pa ridërgim automatik. Fiskalizimi i ri kërkon pagesa reale që përputhen saktësisht; rregulli i vjetër i pagesës cash automatike nuk zbatohet në rrugën e re strikte. Shih [SEF_ATK_MJEKU_IMPLEMENTATION_2026_10_26.md](SEF_ATK_MJEKU_IMPLEMENTATION_2026_10_26.md).

Kontrata e thirrjeve: `pos-native-windows/docs/MJEKU_SEF_BRIDGE_CONTRACT.md` (v1).
Plani: `Desktop\Plani 2026\Plani i integrimit te Mjeku brenda PROGRAMERI POS\INTEGRIMI_I_MJEKUT_ME_SEF.md`.

## Për përdoruesin

### Çfarë është

Mjeku mbetet program më vete. Shtohet vetëm një motor fiskal i ri: **SEF (PROGRAMERI POS)**. Kur zgjidhet, Mjeku nuk krijon skedarë `.inp` dhe nuk printon vetë kuponin: ia dërgon shitjen PROGRAMERI POS-it, që krijon kuponin SEF, e ruan dhe e dërgon në ATK.

ENTERNET, Global EU dhe "SEF (Printer Windows)" mbeten siç ishin.

### Si zgjidhet

1. Hape Mjekun **nga PROGRAMERI POS** (moduli "Mjeku"). Vetëm kështu Mjeku mund të flasë me shërbimin fiskal.
2. Cilësimet → **Printeri fiskal** → zgjidh **SEF (PROGRAMERI POS)** → shtyp **Konfiguro SEF**.
3. Konfirmo mesazhin. Hapet konfigurimi SEF i PROGRAMERI POS (biznesi, NUI, TVSH, certifikata, arka, printeri…).
4. Kur konfigurimi përfundon dhe SEF është gati, Mjeku e ruan SEF si motor fiskal. Nëse e mbyll konfigurimin pa e përfunduar, motori mbetet ai që ishte.

Zgjedhja **SEF (PROGRAMERI POS)** shfaqet vetëm kur:

- Mjeku është në Windows ose Android (në macOS/iOS nuk shfaqet kurrë), **dhe**
- binari desktop i Mjekut e ka urën SEF (aftësia `sef_programeri_bridge_v1`), **dhe**
- Mjeku u nis nga PROGRAMERI POS (ekzistojnë `PROGRAMERI_FISCAL_PIPE` dhe `PROGRAMERI_FISCAL_TOKEN`) — **ose** SEF (PROGRAMERI POS) është tashmë motori i ruajtur.

Një përdorues që e hap Mjekun vetë (pa PROGRAMERI POS) nuk e sheh fare zgjedhjen e re. Nëse SEF (PROGRAMERI POS) është tashmë i ruajtur dhe Mjeku hapet vetë, Cilësimet tregojnë "Hape Mjekun nga PROGRAMERI POS." dhe fiskalizimi refuzohet me të njëjtin mesazh.

### Çfarë ndryshon pas zgjedhjes

- **Fiskalizimi** (butonat ekzistues te Shitjet, Statistika, Dhëmbët; radha fiskale nga telefoni) dërgon shitjen te PROGRAMERI POS. Në sukses fatura shënohet e fiskalizuar si më parë dhe ruhet numri i kuponit SEF. Në gabim shfaqet mesazhi i PROGRAMERI POS dhe fatura **nuk** shënohet e fiskalizuar.
- **X Raport / Z Raport** (shiriti sipër) merren nga PROGRAMERI POS. Z Raporti printohet nga PROGRAMERI POS.
- Në menu shfaqet **Kuponët**: kuponët e ditës (ose të një date), statusi, **Kopje**, **Kthe**, **Anulo**, **X Raport**, dhe baneri Online/Offline/48 orë që rifreskohet çdo 3 sekonda.
- Te Cilësimet shfaqen fushat **TVSH për SEF** (shih "TVSH" më poshtë).

### Kthimi prapa nuk lejohet

Pasi SEF (PROGRAMERI POS) ruhet, kjo arkë nuk mund të kthehet te ENTERNET, Global EU ose SEF (Printer Windows). Mesazhi:

> Kjo arkë punon me SEF (PROGRAMERI POS). Kthimi te ENTERNET, Global EU ose SEF (Printer Windows) nuk lejohet, sepse e njëjta pikë shitjeje nuk mund të bëjë edhe PEF edhe SEF. Për ndryshim kontakto mbështetjen e PROGRAMERI POS.

Butoni **Hap konfigurimin SEF** e rihap konfigurimin pa e ndryshuar motorin.

### Mesazhet

| Kur | Mesazhi |
| --- | --- |
| Mjeku nuk u hap nga PROGRAMERI POS | Hape Mjekun nga PROGRAMERI POS. |
| Shërbimi fiskal nuk përgjigjet / pipe mungon | Shërbimi fiskal i PROGRAMERI POS nuk po përgjigjet. Mbyll Mjekun dhe hape përsëri nga PROGRAMERI POS. |
| Lidhja u ndërpre gjatë thirrjes | Lidhja me PROGRAMERI POS u ndërpre. Kontrollo listën "Kuponët" dhe provoje përsëri. |
| S'ka përgjigje brenda 30 s | PROGRAMERI POS nuk u përgjigj në kohë. Kontrollo listën "Kuponët" para se ta provosh përsëri (e njëjta faturë nuk krijon kupon të dytë). |
| Përgjigje e pakuptueshme | Përgjigje e pavlefshme nga PROGRAMERI POS. Mbyll Mjekun dhe hape përsëri nga PROGRAMERI POS. |
| Konfigurimi s'përfundoi | Konfigurimi SEF nuk përfundoi. Motori fiskal mbeti i pandryshuar. |
| Platformë pa SEF | SEF (PROGRAMERI POS) punon vetëm në Windows dhe Android. |
| Android (ende) | SEF (PROGRAMERI POS) në Android nuk është ende i lidhur. |
| Gabimet e PROGRAMERI POS (`SEF_LOCKED_48H`, `OFFLINE_CAP_REACHED`, `INTEGRITY_BLOCKED`, `APPROVAL_PENDING`, `VALIDATION`, `CONFLICT`, `NOT_FOUND`, `NOT_ALLOWED`, `UNAUTHORIZED`, `NOT_CONFIGURED`, `INTERNAL`) | Tregohet ashtu siç vjen (`error.message`, shqip). |
| Sukses | Kuponi SEF nr. 12 u lëshua (25.00 €). — me shtesë "SEF është offline: kuponi u ruajt dhe dërgohet automatikisht në ATK." kur statusi është `PENDING_OFFLINE`. |

Kufizim i njohur (UI ekzistuese, e pandryshuar qëllimisht): pas fiskalizimit të suksesshëm me SEF, Shitjet tregojnë mesazhin e vjetër "Kuponi fiskal u dërgua për printim." dhe mesazhin e plotë me numrin e kuponit e kopjojnë në clipboard (aty ku ENTERNET kopjonte path-in e `.inp`). Pas një gabimi, dritarja e vjetër "Ribëj" përmend printerin. Numri i kuponit shihet te **Kuponët**.

Kujdes: pas një gabimi lidhjeje ose kohe, ekrani ekzistues i fiskalizimit ofron "Ribëj" dhe pastaj "Procedo si jo fiskale". **Pas një rezultati të pasigurt nuk lejohet ridërgim automatik.** Hap **Kuponët → Historiku lokal i kërkesave SEF → Verifiko në POS**. Fatura me tentativë SEF nuk mund të shënohet manualisht fiskale ose jo-fiskale pa rikuperim/kontroll. Ky kontroll kërkon versionin e ri të të dy aplikacioneve.

## Për zhvilluesin

### Vlera e ruajtur: `sef_programeri`

`fiscal_printer_provider` kishte tashmë vlerën `sef` = "SEF (Printer Windows)": printim i faturës PDF në një printer Windows (`sef_printer_name`), në kod që nga v0.4.9/0.4.10 dhe i dukshëm te Cilësimet. Klientët ekzistues mund ta përdorin, prandaj **nuk u ripërdor**. Motori i ri ruhet si **`sef_programeri`** (pranohen edhe `sef_programeri_pos`, `programeri_sef` → normalizohen në `sef_programeri`). Fjala "`sef`" te kontrata nënkupton motorin SEF, jo vlerën e ruajtur.

### Aftësia `sef_programeri_bridge_v1` (UI ↔ binari)

`get_app_info` kthen fushën e re `capabilities: string[]`. Binari i ri raporton `["sef_programeri_bridge_v1"]` në Windows/Android dhe `[]` në macOS/iOS. UI-ja e re (`hasSefBridge(info)` te `api/types.ts`) shfaq zgjedhjen SEF (PROGRAMERI POS), fushat "TVSH për SEF", menunë/faqen **Kuponët** (me banerin Online/Offline/48h) dhe thërret `sef_bridge_info` **vetëm** kur aftësia ekziston. Pa të (binar i vjetër që s'e ka fushën), UI-ja e re sillet saktësisht si UI-ja e sotme: asnjë zgjedhje e re, asnjë faqe e re, asnjë thirrje e komandave të reja.

### Skedarët

| Skedari | Çfarë |
| --- | --- |
| `src-tauri/src/sef_bridge.rs` | Klienti i urës: seanca nga env, kornizimi 4 bajt LE + JSON (maks. 1 MiB), mbështjellja `v/id/token/op/params`, transporti named pipe (Windows), stub Android, cent/TVSH/pagesat, teste. |
| `src-tauri/src/lib.rs` | `normalize_fiscal_printer_provider` (+`sef_programeri`), mbrojtja te `settings_set`, degët SEF te `invoice_export_fiscal_inp`, `fiscal_report_x_inp`, `fiscal_report_z_inp`, `fiscal_job_process_inner`; komandat e reja. |
| `src-tauri/src/models.rs` | `AppInfo.capabilities` (`#[serde(default)]`). |
| `src-tauri/src/db.rs` | Kolonat lokale te `sales`; `sef_collect_sale`, `sef_mark_fiscalized`, `sef_sale_coupon`. |
| `src-tauri/Cargo.toml` | tokio: shtohen veçoritë `net` dhe `io-util` (named pipe). |
| `mjeku-ui`: `SettingsPage.tsx`, `CouponsPage.tsx`, `App.tsx`, `Sidebar.tsx`, `api/client.ts`, `api/types.ts` | Zgjedhja, konfigurimi, faqja Kuponët. |

Rruga ENTERNET (`fiscal_receipt_generate_inp`, `C:\Temp`, `.inp/.out`) nuk është prekur; degët SEF kthehen para saj.

### Komandat Tauri

| Komanda | Thirrja | Leje |
| --- | --- | --- |
| `sef_bridge_info` | — (platforma, a u nis nga PROGRAMERI POS, motori) | i kyçur |
| `sef_bridge_status` | `status` | i kyçur |
| `sef_bridge_select_engine` | `status` + `configure` (pa kufi praktik, 8 orë) → ruan `sef_programeri` vetëm me `ready:true`; nëse SEF është tashmë motori, vetëm rihap konfigurimin | pronar / mjek admin (jo arkëtar) |
| `invoice_export_fiscal_inp` (ekzistuese) | `fiscalize` | financa |
| `fiscal_report_x_inp` / `fiscal_report_z_inp` (ekzistuese) | `report.x` / `report.z` (data e sotme) | financa |
| `sef_coupons_list` | `coupons.list` (`from`/`to` opsionale) | financa + motori SEF |
| `sef_coupon_copy` | `coupon.copy` | financa + motori SEF |
| `sef_coupon_return` | `coupon.return` (artikujt `{id, qty}`, arsyeja) | financa + motori SEF |
| `sef_coupon_cancel` | `coupon.cancel` (arsyeja) | financa + motori SEF |
| `sef_report` | `report.x` / `report.z` me tekst të plotë | financa + motori SEF |
| `sef_sale_items` | — (rreshtat e fiskalizuar të shitjes, për "Kthe") | financa |
| `sef_sale_coupon` | — (kuponi i ruajtur lokalisht) | financa |

Kohëzgjatjet: 30 s për çdo thirrje; `configure` 8 orë. Lidhja hapet për çdo thirrje; kur pipe është i zënë (`ERROR_PIPE_BUSY`) provohet deri në 5 s. `ClientOptions` përdor `SECURITY_IDENTIFICATION` (serveri e identifikon thirrësin, nuk e imiton). Emri i pipe-it pranohet vetëm si emër i thjeshtë (`A–Z a–z 0–9 - _ .`, pa `..`), që të mos drejtohet te pipe në rrjet. Tokeni nuk shkruhet në log.

### Fiskalizimi (`fiscalize`)

- `source` = `mjeku`, `saleId` = `sales.id`, `issuedAt` = ora lokale ISO-8601 me offset.
- Rreshtat zgjidhen si te ENTERNET: `visit_items` me `fiscal=1, fiscalized=0, deleted=0`; nëse s'ka dhe thirrja lejon (`allow_any_if_no_fiscal`, që UI e dërgon `true`), të gjithë rreshtat e pafiskalizuar (pastaj shënohen `fiscal=1`).
- `buyer` nuk dërgohet (pacientë = persona fizikë; kontrata e lejon pa blerës).
- Një fiskalizim në një kohë për proces (mutex), pa mbajtur bllokun e databazës gjatë thirrjes.
- Në sukses (`FISCALIZED` ose `PENDING_OFFLINE`), në një transaksion: `visit_items.fiscalized=1`, `sales.fiscalized=1`, radha e sinkronizimit si te ENTERNET, pagesa automatike `cash` me shënimin `[AUTO-FISCAL] Kupon fiskal SEF` vetëm kur shitja s'ka asnjë pagesë, dhe kolonat lokale `sales.sef_coupon_id`, `sef_daily_number`, `sef_status`, `sef_verification_url`, `sef_fiscalized_at` (shtohen me `add_column_if_missing`; nuk sinkronizohen në Supabase sepse `Sale` s'i ka dhe upsert-et përdorin kolona të emëruara).
- Në gabim nuk shënohet asgjë.
- Nëse kuponi u lëshua por shënimi lokal dështoi, mesazhi e thotë dhe ribërja kthen të njëjtin kupon (`duplicate:true`).

### Rregulli i parave (cent)

1. Çmimi për njësi (REAL, euro) → cent me rrumbullakim **half away from zero**, me epsilon 1e-6 kundër gabimeve të f64 (p.sh. 2.675 → 268, 1.005 → 101).
2. Sasia → mijëshe (3 decimale, e njëjta rrumbullakim) dhe dërgohet si tekst pa zero në fund (`"1"`, `"0.5"`, `"1.25"`).
3. Totali i rreshtit = sasia × çmimi për njësi në cent, me numra të plotë, half away from zero (0.5 × 333 = 166.5 → 167).
4. Sasi e pavlefshme (≤ 0, NaN) → 1; çmim i pavlefshëm (< 0, NaN) → 0 — njësoj si rruga ENTERNET.
5. Totali = shuma e totaleve të rreshtave.

### Rregulli i pagesave (shuma e pagesave = shuma e artikujve)

1. Merren pagesat e shitjes (`payments`, jo të fshira, shumë > 0), kthehen në cent dhe grupohen sipas metodës: `cash`→`cash`, `card`→`card`, `bank`→`transfer`, të tjerat→`other`.
2. Nëse shuma e tyre është **saktësisht** totali i artikujve, dërgohen ashtu.
3. **Nga 26.10.2026, rruga e re strikte SEF refuzon pagesën e pjesshme ose mospërputhjen.** Nuk fabrikohet pagesë me metodën dominuese.
4. **Pa pagesë reale të regjistruar, fiskalizimi refuzohet.** Regjistro pagesën përpara SEF. Funksioni historik i konvertimit mbetet në kod për testet e kompatibilitetit, por nuk thirret nga `sef_fiscalize_blocking`.
5. Para dërgimit kontrollohet që artikujt = pagesat = totali; përndryshe nuk dërgohet.

### TVSH

Mjeku ruan kodin e TVSH-së te çdo rresht (`vat_code`). Rruga ENTERNET e kthen kodin në grup printeri (`A→1, C→3, D→4, E→fiscal_vat_e_group`), jo në përqindje, prandaj përqindjet merren nga etiketat e Mjekut (Shërbimet/Cilësimet):

| Kodi | Kuptimi te Mjeku | `vatRatePercent` parazgjedhur | Cilësimi për ndryshim |
| --- | --- | --- | --- |
| A | 0% e përjashtuar | 0 | `sef_vat_rate_a` |
| C | 0% – biznes jo në TVSH | 0 | `sef_vat_rate_c` |
| D | 8% | 8 | `sef_vat_rate_d` |
| E | 18% | 18 | `sef_vat_rate_e` |
| tjetër/bosh | **Refuzohet në rrugën e re strikte SEF** (nuk supozohet normë 0%) | — | — |

Cilësimet pranojnë numër të plotë 0–100 (lejohet `%` në fund); bosh ose i pavlefshëm → parazgjedhja. Ndryshohen te Cilësimet → "TVSH për SEF".

### Android

`sef_bridge.rs` ka transport stub për `target_os = "android"` që refuzon çdo thirrje ("…në Android nuk është ende i lidhur."). TODO (Faza 4): plugin Tauri mobile (Kotlin) që lidhet me shërbimin e PROGRAMERI POS Mobile (bound service me leje dhe kontroll nënshkrimi) dhe kalon të njëjtat mesazhe JSON. Deri atëherë, në telefon fiskalizimi vazhdon me radhën fiskale ekzistuese, që e proceson PC-ja e klinikës (me SEF, përmes PROGRAMERI POS).

## Si provohet

### Teste automatike

```
cargo test --manifest-path src-tauri/Cargo.toml sef_bridge
```

Testet mbulojnë kornizimin (LE, kufiri 1 MiB), mbështjelljen, përgjigjet ok/gabim/id/version, variablat e seancës, centët, sasitë, totalet e rreshtave, TVSH-në me mbishkrime, pagesat (përputhje, mospërputhje, pa pagesa), kontrollin e shumave, formën e `fiscalize`, dhe në Windows një server named pipe të vërtetë: roundtrip, pipe që mungon (`NOT_RUNNING`), server i heshtur (`TIMEOUT`), dhe thirrja nga `spawn_blocking` me runtime të ri (si te `lib.rs`).

### Me PROGRAMERI POS (ambient TEST)

1. Në PROGRAMERI POS, ambienti TEST, hap modulin "Mjeku". Për të provuar shërbimin pa Mjekun, përdor programin e testit `pos-native-windows/tools/Programeri.MjekuBridge.TestClient` (`MjekuBridgeTestClient.exe`): nise nga moduli Mjeku me "Zgjidh programin…", që t'i japë `PROGRAMERI_FISCAL_PIPE/TOKEN/CONTEXT`. Pa argumente hap një menu; `--all` ekzekuton të gjitha thirrjet (pa `configure` dhe pa `report.z`); `--configure` dhe `--z` i shtojnë ato. Fiskalizon vetëm kur `status.environment` është `TEST`.
2. Pa PROGRAMERI POS: nis Mjekun vetë → te Cilësimet **nuk** shfaqet "SEF (PROGRAMERI POS)", nuk ka "Kuponët", dhe gjithçka duket si në versionin e mëparshëm. Nëse motori SEF (PROGRAMERI POS) ishte ruajtur më parë → "Hape Mjekun nga PROGRAMERI POS." dhe asnjë fiskalizim.
3. Nga PROGRAMERI POS: Konfiguro SEF → mbylle pa përfunduar → motori mbetet i pandryshuar. Përfundoje → motori bëhet SEF (PROGRAMERI POS).
4. Provo kthimin te ENTERNET → refuzohet me mesazhin më lart.
5. Fiskalizo një shitje → nuk krijohet asnjë `.inp` te `C:\Temp`; kuponi shfaqet te Kuponët me të njëjtin numër si te PROGRAMERI POS; fatura shënohet e fiskalizuar.
6. Ribëj fiskalizimin e së njëjtës faturë (p.sh. pas një gabimi) → i njëjti kupon.
7. Shkëput rrjetin → kuponi `PENDING_OFFLINE`, baneri offline te Kuponët; bllokimi 48 orë / kufiri offline → fiskalizimi refuzohet me mesazhin e PROGRAMERI POS dhe fatura mbetet e pafiskalizuar.
8. Kopje, Kthe (artikull + sasi + arsye), Anulo (arsye), X/Z → krahaso me PROGRAMERI POS.
9. Me ENTERNET në një PC tjetër: fiskalizimi krijon `.inp` si më parë.

Variablat e seancës, që PROGRAMERI POS i vendos kur e nis Mjekun: `PROGRAMERI_FISCAL_PIPE`, `PROGRAMERI_FISCAL_TOKEN`, `PROGRAMERI_FISCAL_CONTEXT`. Për provë me dorë kundër një serveri testi mund të vendosen në një dritare komandash para se të niset `mjeku_desktop.exe` — vetëm në ambient TEST.

## Çfarë është provuar (15 tetor 2026)

- `sef_bridge.rs`: 19 teste kaluan me Rust 1.99 (GNU toolchain) në një crate të veçantë me varësi të njëjta (versione më të vjetra), përfshirë named pipe të vërtetë në Windows; clippy pa vërejtje për modulin.
- `mjeku-ui`: `npm run typecheck` dhe `npm run build` kaluan.
- **Nuk** u ndërtua crate-i i plotë `mjeku_desktop` (`lib.rs`, `db.rs`): mungojnë MSVC Build Tools / kompajler C në këtë PC. Kodi është shkruar me kujdes, por duhet `cargo check`/`cargo clippy`/`cargo test` në një makinë me toolchain të plotë para çdo build-i.
- Nuk u provua kundër shërbimit real të PROGRAMERI POS.

## Klientët ekzistues të Mjekut

Mjeku standalone është i instaluar te klientë dhe përditësohet vetë, në dy pjesë të ndara:

- **Binari desktop** (`desktop_updates.rs`, Tauri updater): `desktop-update.json` nga GitHub Releases / Vercel.
- **Paketa e UI-së** (`updates.rs`, `ui_protocol.rs`): `manifest.json` nga `mjeku-ui.onrender.com` (rezervë Vercel), çdo 6 orë. `manifest.json` ka vetëm `latestVersion` (hash i git-it të `mjeku-ui`), `publishedAt`, `sha256`, `bundlePath` — **nuk ka "versioni minimal i binarit"**. Pra çdo UI e publikuar mund t'i shkojë çdo binari, të vjetër ose të ri. Binari ka edhe një UI "seed" brenda instaluesit (`resources/ui-seed.zip`, e ndërtuar nga `mjeku-ui` në kohën e build-it të desktopit).

### Mospërputhja e versioneve

| Rasti | Çfarë ndodh |
| --- | --- |
| UI e re + binar i vjetër | Binari i vjetër s'kthen `capabilities` → UI-ja e re s'shfaq asgjë të re dhe s'thërret asnjë komandë të re. Duket dhe punon si sot. |
| UI e vjetër + binar i ri | UI-ja e vjetër s'i njeh komandat e reja dhe s'i thërret; fusha `capabilities` injorohet. Parazgjedhjet janë të pandryshuara (shih më poshtë). Rast i vetëm i veçantë: nëse SEF (PROGRAMERI POS) ishte ruajtur më parë, UI-ja e vjetër e tregon motorin si "ENTERNET" (s'e njeh vlerën), por fiskalizimi dhe X/Z shkojnë te PROGRAMERI POS (vendos backend-i), dhe "Ruaj" i një motori tjetër refuzohet me mesazhin e kthimit prapa. |
| UI e re + binar i ri, Mjeku i hapur vetë | Zgjedhja e re nuk shfaqet (s'ka variabla seance). |
| UI e re + binar i ri, nga PROGRAMERI POS | Zgjedhja shfaqet; "Kuponët" vetëm pasi SEF (PROGRAMERI POS) ruhet. |

### Rendi i publikimit (i detyrueshëm)

1. **Së pari binari desktop** (me aftësinë `sef_programeri_bridge_v1`). Instaluesi i tij përmban tashmë UI-në e re si "seed", ndaj veçoria punon edhe pa publikuar UI veç.
2. **Pastaj paketa e UI-së** në `mjeku-ui.onrender.com`/Vercel. Falë aftësisë, kjo është e sigurt edhe për klientët që s'e kanë përditësuar ende binarin, por para publikimit verifikoje UI-në e re me një binar të vjetër.

Asnjë nga këto nuk është bërë në këtë punë (pa commit, pa version, pa publikim).

### Cilësimet dhe të dhënat ekzistuese

- **Parazgjedhja e motorit** mbetet `enternet` (`get_fiscal_printer_provider`: bosh → `enternet`), e pandryshuar.
- **ENTERNET, Global EU dhe `sef` (Printer Windows)**: kodi i tyre te `invoice_export_fiscal_inp`, `fiscal_report_x_inp`/`_z_inp`, `fiscal_job_process_inner` dhe `fiscal_receipt_generate_inp` është i pandryshuar; degët e reja janë `if provider == "sef_programeri" { … return }` para tyre. Te `normalize_fiscal_printer_provider` u shtua vetëm një krah për `sef_programeri` (që më parë refuzohej). Te `settings_set` kontrolli i ri vepron vetëm kur vlera aktuale ose e reja është `sef_programeri`; mesazhi i gabimit për vlerë të pavlefshme është i njëjtë.
- **Cilësimet** ruhen te `app_settings` lokale dhe nuk sinkronizohen në Supabase (sinkronizohet vetëm `clinic_bank_account` te `clinic_registry`). Pra `sef_programeri` dhe `sef_vat_rate_*` mbeten te ai PC.
- **Kolonat e reja** `sales.sef_coupon_id`, `sef_daily_number`, `sef_status`, `sef_verification_url`, `sef_fiscalized_at` janë vetëm lokale: `struct Sale` nuk i ka, ndaj nuk hyjnë në payload-et e radhës së sinkronizimit (push) dhe nuk lexohen nga Supabase (pull). `apply_remote_sale` dhe `sales_upsert` përdorin `INSERT … ON CONFLICT(id) DO UPDATE SET` me kolona të emëruara (jo `INSERT OR REPLACE`), ndaj një pull nuk i fshin. Askund nuk përdoret `SELECT * FROM sales`. Supabase nuk ka nevojë për ndryshim skeme.
- **Migrimi** në databaza të vjetra: `add_column_if_missing` kontrollon `pragma_table_info` para `ALTER TABLE … ADD COLUMN`, ndaj ekzekutohet pa dëm në çdo nisje (idempotent). Kolonat janë `TEXT`/`INTEGER` pa `NOT NULL` dhe pa parazgjedhje, ndaj rreshtat ekzistues marrin `NULL`. Një binar më i vjetër që hap të njëjtën databazë i injoron kolonat shtesë.

### Ku gjendet instalimi (për PROGRAMERI POS)

Nga `tauri.conf.json` (`productName: "Mjeku"`, `identifier: "com.mjeku.desktop"`, `bundle.targets: ["nsis"]`, `nsis.installMode: "currentUser"`, pa `mainBinaryName`) dhe shablloni NSIS i Tauri CLI 2.10.0 (versioni te `package-lock.json`):

| Çfarë | Vlera |
| --- | --- |
| Dosja e paracaktuar | `%LOCALAPPDATA%\Mjeku` (p.sh. `C:\Users\<emri>\AppData\Local\Mjeku`); përdoruesi mund ta ndryshojë gjatë instalimit, prandaj lexo regjistrin. |
| Exe | `mjeku_desktop.exe` (emri i paketës Cargo; Tauri e ruan emrin e binarit të cargo kur s'ka `mainBinaryName`). |
| Çelësi i çinstalimit | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Mjeku` (`currentUser` → `SHCTX` = HKCU) |
| `DisplayName` | `Mjeku` |
| `DisplayVersion` | versioni nga `tauri.conf.json`, sot `0.5.16` |
| `MainBinaryName` | `mjeku_desktop.exe` |
| `InstallLocation` | dosja e instalimit **me thonjëza** (`"C:\…\Mjeku"`) — hiqi thonjëzat |
| `DisplayIcon` | `"<dosja>\mjeku_desktop.exe"` (me thonjëza) |
| `Publisher` | `mjeku` (pjesa e dytë e identifier-it) |
| Çelës shtesë | `HKCU\Software\mjeku\Mjeku`, vlera (Default) = dosja e instalimit |

Rekomandim: gjej exe-në me `InstallLocation` + `MainBinaryName` nga çelësi i çinstalimit (jo me path të ngulitur). Këto vlera janë nxjerrë nga shablloni dhe konfigurimi, **jo** nga një instalim real (Mjeku s'është i instaluar në PC-në e zhvillimit).

## Çështje të hapura për kontratën

- Fjala `sef` te kontrata: vlera e ruajtur te Mjeku është `sef_programeri` (shih më lart).
- `coupons.list` nuk kthen artikujt e kuponit; për "Kthe" Mjeku merr rreshtat e fiskalizuar nga databaza e vet (`visit_items` me `saleId`), por nuk di sasinë e mbetur pas kthimeve të mëparshme — PROGRAMERI POS e kontrollon (`NOT_ALLOWED`). Një `coupon.get`/artikujt te lista do ta saktësonte.
- `vatRatePercent` nuk dallon "0% e përjashtuar" (A) nga "0% jo në TVSH" (C). Nëse kuponi SEF kërkon këtë dallim, duhet fushë shtesë (p.sh. `vatCode`/`exempt`).
- Rregulli i rrumbullakimit për sasi me decimale (sasia × çmimi për njësi) duhet të jetë i njëjtë në PROGRAMERI POS; Mjeku përdor half away from zero në cent.


## Kontrolli i rikuperimit (26.10.2026)

MJEKU ruan kërkesën fiskale dhe hash-in përpara dërgimit te POS. Pas timeout-it, ndërprerjes ose gabimit të protokollit, statusi mbetet `UNKNOWN`. `sale.lookup` kontrollon kuponin origjinal në POS pa emetim të ri. `NOT_FOUND` nuk autorizon ridërgim automatik. Auditimi i ngjarjeve është append-only. Për testet dhe kërkesat e ATK, shih dokumentin e implementimit të 26.10.2026.
