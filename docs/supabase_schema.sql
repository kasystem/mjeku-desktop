-- Mjeku Supabase schema (run in Supabase SQL editor)
-- Tables: clients, sales, payments, doctors, services, appointments, visits, visit_items, cash_ledger
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

create table if not exists public.doctors (
  id uuid primary key,
  name text not null,
  phone text,
  email text,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create table if not exists public.services (
  id uuid primary key,
  title text not null,
  default_price double precision not null,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create table if not exists public.appointments (
  id uuid primary key,
  client_id uuid not null,
  doctor_id uuid,
  start_at timestamptz not null,
  end_at timestamptz,
  status text not null,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create table if not exists public.visits (
  id uuid primary key,
  client_id uuid not null,
  doctor_id uuid,
  date date,
  status text not null,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create table if not exists public.visit_items (
  id uuid primary key,
  visit_id uuid not null,
  client_id uuid not null,
  tooth text,
  title text not null,
  qty double precision not null,
  unit_price double precision not null,
  fiscal integer not null default 1,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create table if not exists public.cash_ledger (
  id uuid primary key,
  type text not null,
  date date,
  amount double precision not null,
  category text,
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

create index if not exists doctors_updated_at_idx on public.doctors(updated_at);
create index if not exists services_updated_at_idx on public.services(updated_at);

create index if not exists appointments_client_id_idx on public.appointments(client_id);
create index if not exists appointments_doctor_id_idx on public.appointments(doctor_id);
create index if not exists appointments_start_at_idx on public.appointments(start_at);
create index if not exists appointments_status_idx on public.appointments(status);
create index if not exists appointments_updated_at_idx on public.appointments(updated_at);

create index if not exists visits_client_id_idx on public.visits(client_id);
create index if not exists visits_doctor_id_idx on public.visits(doctor_id);
create index if not exists visits_date_idx on public.visits(date);
create index if not exists visits_status_idx on public.visits(status);
create index if not exists visits_updated_at_idx on public.visits(updated_at);

create index if not exists visit_items_visit_id_idx on public.visit_items(visit_id);
create index if not exists visit_items_client_id_idx on public.visit_items(client_id);
create index if not exists visit_items_updated_at_idx on public.visit_items(updated_at);

create index if not exists cash_ledger_type_idx on public.cash_ledger(type);
create index if not exists cash_ledger_date_idx on public.cash_ledger(date);
create index if not exists cash_ledger_updated_at_idx on public.cash_ledger(updated_at);

-- RLS guidance:
-- Option A (simplest): disable RLS on these tables.
--   alter table public.clients disable row level security;
--   alter table public.sales disable row level security;
--   alter table public.payments disable row level security;
--   alter table public.doctors disable row level security;
--   alter table public.services disable row level security;
--   alter table public.appointments disable row level security;
--   alter table public.visits disable row level security;
--   alter table public.visit_items disable row level security;
--   alter table public.cash_ledger disable row level security;
--
-- Option B: enable RLS and allow anon read/write (only if this project is private to the clinic).
--   alter table public.clients enable row level security;
--   create policy "anon_all_clients" on public.clients for all to anon using (true) with check (true);
--   alter table public.sales enable row level security;
--   create policy "anon_all_sales" on public.sales for all to anon using (true) with check (true);
--   alter table public.payments enable row level security;
--   create policy "anon_all_payments" on public.payments for all to anon using (true) with check (true);
--   alter table public.doctors enable row level security;
--   create policy "anon_all_doctors" on public.doctors for all to anon using (true) with check (true);
--   alter table public.services enable row level security;
--   create policy "anon_all_services" on public.services for all to anon using (true) with check (true);
--   alter table public.appointments enable row level security;
--   create policy "anon_all_appointments" on public.appointments for all to anon using (true) with check (true);
--   alter table public.visits enable row level security;
--   create policy "anon_all_visits" on public.visits for all to anon using (true) with check (true);
--   alter table public.visit_items enable row level security;
--   create policy "anon_all_visit_items" on public.visit_items for all to anon using (true) with check (true);
--   alter table public.cash_ledger enable row level security;
--   create policy "anon_all_cash_ledger" on public.cash_ledger for all to anon using (true) with check (true);
