# Printer selection and VAT categories — 2026-10-20

Windows: `printers_list` reads installed Windows spooler queues on a worker,
without opening a console. Settings offers a selector, refresh, error and empty
states, and retains a saved printer that is temporarily unavailable. Physical
print output still requires a device test; enumeration is not print evidence.

Android: BLOCKED for native printer discovery/selection in Mjeku. This checkout
has no generated Android host and no Android printing adapter. Follow-up: add
the native adapter and permission flow, connect selection to a real print
transport (Bluetooth/USB/LAN or Android PrintManager as appropriate), and test
on a device. Do not claim parity with PROGRAMERI POS or save a Windows queue
name as if it were an Android printer. The current native command returns an
explicit unsupported-platform error outside Windows.

macOS / iOS: native printer selection also BLOCKED pending platform adapters
and device verification. The shared UI must not imply that Windows spooler
enumeration works on these platforms.

VAT service and visit-item writes retain A/C/D/E for a registered business;
an unregistered business uses C only. Empty new categories default to A/C
respectively. The startup Visit service already uses these defaults. Existing
historical records are not rewritten. Shared Rust/React code applies to the
platform builds; only Windows can be built and launched in this session.

The separate PROGRAMERI POS fiscal bridge currently transmits percentages,
not the A/C distinction. Preserving both zero-rate categories end to end in
that bridge remains follow-up work; these catalog changes do not prove fiscal
bridge parity or ATK acceptance.
