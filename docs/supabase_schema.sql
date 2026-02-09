-- Mjeku Supabase schema (run in Supabase SQL editor)
-- Tables: clients, sales, payments
-- Notes:
-- - This keeps `deleted` as an integer (0/1) to match the local SQLite schema.
-- - You can either disable RLS (simplest for single-clinic project) or add permissive policies for anon.

create extension if not exists "pgcrypto";

create table if not exists public.clients (
  id uuid primary key,
  name text not null,
  phone text,
  email text,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create table if not exists public.sales (
  id uuid primary key,
  client_id uuid not null,
  date date,
  total double precision not null,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create table if not exists public.payments (
  id uuid primary key,
  client_id uuid not null,
  sale_id uuid,
  date date,
  amount double precision not null,
  method text,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create index if not exists clients_updated_at_idx on public.clients(updated_at);
create index if not exists sales_client_id_idx on public.sales(client_id);
create index if not exists sales_date_idx on public.sales(date);
create index if not exists sales_updated_at_idx on public.sales(updated_at);
create index if not exists payments_client_id_idx on public.payments(client_id);
create index if not exists payments_sale_id_idx on public.payments(sale_id);
create index if not exists payments_date_idx on public.payments(date);
create index if not exists payments_updated_at_idx on public.payments(updated_at);

-- RLS guidance:
-- Option A (simplest): disable RLS on these tables.
--   alter table public.clients disable row level security;
--   alter table public.sales disable row level security;
--   alter table public.payments disable row level security;
--
-- Option B: enable RLS and allow anon read/write (only if this project is private to the clinic).
--   alter table public.clients enable row level security;
--   create policy "anon_all_clients" on public.clients for all to anon using (true) with check (true);
--   alter table public.sales enable row level security;
--   create policy "anon_all_sales" on public.sales for all to anon using (true) with check (true);
--   alter table public.payments enable row level security;
--   create policy "anon_all_payments" on public.payments for all to anon using (true) with check (true);

