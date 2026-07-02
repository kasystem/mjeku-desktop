-- Mjeku v0.4.9 — Ofertat (offers + offer_items) Supabase schema
-- Run in the Supabase SQL editor (project: occzpzryzxabajtmdaas).
--
-- STATUS (verified 2026-07-02 against the live project):
--   * Both tables ALREADY EXIST with all columns below.
--   * BUT RLS is enabled with NO policies -> anon INSERT fails with 42501
--     ("new row violates row-level security policy") and SELECT returns
--     empty rows, so desktop sync for Ofertat silently does nothing.
--   * The operative fix is the RLS section at the bottom. The CREATE
--     statements are kept idempotent for fresh projects.

create extension if not exists "pgcrypto";

create table if not exists public.offers (
  id uuid primary key,
  clinic_id uuid not null,
  client_id uuid not null,
  offer_number text not null default '',
  status text not null default 'draft', -- draft|sent|accepted|rejected|invoiced
  valid_until date,
  notes text,
  vat_pct double precision not null default 18,
  subtotal double precision not null default 0,
  vat_amount double precision not null default 0,
  total double precision not null default 0,
  invoice_id uuid,
  source_offer_id uuid,
  created_at timestamptz,
  updated_at timestamptz,
  deleted_at timestamptz
);

create table if not exists public.offer_items (
  id uuid primary key,
  offer_id uuid not null,
  clinic_id uuid not null,
  description text not null default '',
  qty double precision not null default 1,
  unit_price double precision not null default 0,
  discount_pct double precision not null default 0,
  line_total double precision not null default 0,
  sort_order integer not null default 0,
  created_at timestamptz,
  updated_at timestamptz,
  deleted_at timestamptz
);

create index if not exists offers_clinic_id_idx on public.offers(clinic_id);
create index if not exists offers_client_id_idx on public.offers(client_id);
create index if not exists offers_status_idx on public.offers(status);
create index if not exists offers_updated_at_idx on public.offers(updated_at);
create index if not exists offer_items_offer_id_idx on public.offer_items(offer_id);
create index if not exists offer_items_clinic_id_idx on public.offer_items(clinic_id);
create index if not exists offer_items_updated_at_idx on public.offer_items(updated_at);

-- ---------------------------------------------------------------------------
-- RLS FIX (this is the part that still needs to run).
-- Option A — match the rest of the project (anon key reads/writes work on
-- clients/sales/visits/... , so keep offers consistent):
alter table public.offers disable row level security;
alter table public.offer_items disable row level security;

-- Option B — if you prefer RLS enabled with permissive anon policies instead,
-- comment out Option A above and run:
--   alter table public.offers enable row level security;
--   create policy "anon_all_offers" on public.offers
--     for all to anon using (true) with check (true);
--   alter table public.offer_items enable row level security;
--   create policy "anon_all_offer_items" on public.offer_items
--     for all to anon using (true) with check (true);
