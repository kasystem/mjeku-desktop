-- Mjeku Supabase schema (run in Supabase SQL editor)
-- Tables: clients, sales, payments, doctors, services, appointments, visits, visit_items, cash_ledger, app_license, clinic_registry, clinic_tokens
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
  first_name text,
  last_name text,
  parent_name text,
  dob date,
  gender text,
  city text,
  address text,
  allergies text,
  weight_kg double precision,
  height_cm double precision,
  patient_code text,
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
  fiscalized integer not null default 0,
  fiscalized_at timestamptz,
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
  code text,
  name text not null,
  title text,
  specialty text,
  phone text,
  email text,
  notes text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

create unique index if not exists doctors_code_unique_idx
  on public.doctors(code)
  where code is not null and length(trim(code)) > 0;

create table if not exists public.services (
  id uuid primary key,
  title text not null,
  default_price double precision not null,
  vat_code text not null default 'C',
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
  visit_time text,
  status text not null,
  notes text,
  body_weight text,
  body_weight_unit text,
  body_height text,
  body_height_unit text,
  head_circumference text,
  head_circumference_unit text,
  body_temperature text,
  body_temperature_unit text,
  blood_oxygen text,
  blood_oxygen_unit text,
  glycemia text,
  glycemia_unit text,
  pulse text,
  pulse_unit text,
  bmi text,
  blood_pressure_systolic text,
  blood_pressure_diastolic text,
  blood_pressure_unit text,
  complaints text,
  additional_notes text,
  controls text,
  remarks text,
  analyses text,
  advice text,
  therapies text,
  diagnosis text,
  examinations text,
  created_at timestamptz,
  updated_at timestamptz,
  deleted integer not null default 0
);

-- Safe migration for existing projects (keeps old tables compatible).
alter table public.visits add column if not exists visit_time text;
alter table public.visits add column if not exists body_weight text;
alter table public.visits add column if not exists body_weight_unit text;
alter table public.visits add column if not exists body_height text;
alter table public.visits add column if not exists body_height_unit text;
alter table public.visits add column if not exists head_circumference text;
alter table public.visits add column if not exists head_circumference_unit text;
alter table public.visits add column if not exists body_temperature text;
alter table public.visits add column if not exists body_temperature_unit text;
alter table public.visits add column if not exists blood_oxygen text;
alter table public.visits add column if not exists blood_oxygen_unit text;
alter table public.visits add column if not exists glycemia text;
alter table public.visits add column if not exists glycemia_unit text;
alter table public.visits add column if not exists pulse text;
alter table public.visits add column if not exists pulse_unit text;
alter table public.visits add column if not exists bmi text;
alter table public.visits add column if not exists blood_pressure_systolic text;
alter table public.visits add column if not exists blood_pressure_diastolic text;
alter table public.visits add column if not exists blood_pressure_unit text;
alter table public.visits add column if not exists complaints text;
alter table public.visits add column if not exists additional_notes text;
alter table public.visits add column if not exists controls text;
alter table public.visits add column if not exists remarks text;
alter table public.visits add column if not exists analyses text;
alter table public.visits add column if not exists advice text;
alter table public.visits add column if not exists therapies text;
alter table public.visits add column if not exists diagnosis text;
alter table public.visits add column if not exists examinations text;

create table if not exists public.visit_items (
  id uuid primary key,
  visit_id uuid not null,
  client_id uuid not null,
  tooth text,
  title text not null,
  qty double precision not null,
  unit_price double precision not null,
  fiscal integer not null default 1,
  vat_code text not null default 'C',
  fiscalized integer not null default 0,
  fiscalized_at timestamptz,
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

-- License table (singleton row, editable by the vendor/admin)
create table if not exists public.app_license (
  singleton_id int primary key default 1,
  active_until timestamptz,
  disabled boolean not null default false,
  updated_at timestamptz not null default now(),
  constraint app_license_singleton check (singleton_id = 1)
);

insert into public.app_license (singleton_id, active_until, disabled, updated_at)
values (1, now() + interval '30 days', false, now())
on conflict (singleton_id) do nothing;

-- Per-clinic approval, license and IP control (used by vendor web panel + desktop license engine).
create table if not exists public.clinic_registry (
  clinic_id uuid primary key,
  clinic_name text,
  approved boolean not null default false,
  disabled boolean not null default false,
  active_until timestamptz,
  enforce_ip boolean not null default false,
  allowed_ip_list text not null default '',
  last_seen_ip text,
  last_seen_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

-- Onboarding tokens (vendor creates; desktop consumes once or multiple times).
create table if not exists public.clinic_tokens (
  token_code text primary key,
  clinic_id uuid,
  clinic_name text,
  mode text not null default 'new',
  one_time boolean not null default true,
  disabled boolean not null default false,
  expires_at timestamptz,
  used_at timestamptz,
  bootstrap_admin_salt text,
  bootstrap_admin_hash text,
  bootstrap_user_salt text,
  bootstrap_user_hash text,
  bootstrap_cashier_salt text,
  bootstrap_cashier_hash text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint clinic_tokens_mode_check check (mode in ('new', 'existing'))
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

create index if not exists app_license_updated_at_idx on public.app_license(updated_at);
create index if not exists clinic_registry_updated_at_idx on public.clinic_registry(updated_at);
create index if not exists clinic_registry_approved_idx on public.clinic_registry(approved);
create index if not exists clinic_registry_last_seen_at_idx on public.clinic_registry(last_seen_at);
create index if not exists clinic_tokens_clinic_id_idx on public.clinic_tokens(clinic_id);
create index if not exists clinic_tokens_mode_idx on public.clinic_tokens(mode);
create index if not exists clinic_tokens_expires_at_idx on public.clinic_tokens(expires_at);
create index if not exists clinic_tokens_used_at_idx on public.clinic_tokens(used_at);
create index if not exists clinic_tokens_updated_at_idx on public.clinic_tokens(updated_at);

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
--   alter table public.app_license disable row level security;
--   alter table public.clinic_registry disable row level security;
--   alter table public.clinic_tokens disable row level security;
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
--   alter table public.app_license enable row level security;
--   -- Only SELECT for license checks (do not allow anon writes).
--   create policy "anon_select_license" on public.app_license for select to anon using (true);
--   alter table public.clinic_registry enable row level security;
--   -- Desktop clinics read/write only their own clinic_id rows.
--   create policy "anon_select_clinic_registry" on public.clinic_registry for select to anon using (true);
--   create policy "anon_upsert_clinic_registry" on public.clinic_registry for insert to anon with check (true);
--   create policy "anon_update_clinic_registry" on public.clinic_registry for update to anon using (true) with check (true);
--   alter table public.clinic_tokens enable row level security;
--   -- Desktop app finds token by code and marks it used.
--   create policy "anon_select_clinic_tokens" on public.clinic_tokens for select to anon using (true);
--   create policy "anon_insert_clinic_tokens" on public.clinic_tokens for insert to anon with check (true);
--   create policy "anon_update_clinic_tokens" on public.clinic_tokens for update to anon using (true) with check (true);
