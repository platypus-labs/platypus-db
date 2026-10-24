-- Demo schema for PlatypusDB development and integration tests.

CREATE SCHEMA demo;
SET search_path TO demo;

CREATE TABLE customers (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email       text NOT NULL UNIQUE,
    full_name   text NOT NULL,
    country     char(2),
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE orders (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    customer_id  bigint NOT NULL REFERENCES customers (id),
    total        numeric(12, 2) NOT NULL,
    status       text NOT NULL CHECK (status IN ('new', 'paid', 'shipped', 'cancelled')),
    placed_at    timestamptz NOT NULL
);
CREATE INDEX orders_customer_id_idx ON orders (customer_id);

CREATE VIEW customer_totals AS
SELECT c.id, c.full_name, count(o.id) AS orders, coalesce(sum(o.total), 0) AS spent
FROM customers c
LEFT JOIN orders o ON o.customer_id = c.id
GROUP BY c.id, c.full_name;

INSERT INTO customers (email, full_name, country, created_at)
SELECT 'user' || g || '@example.com',
       'Customer ' || g,
       (ARRAY['IT', 'DE', 'FR', 'US', 'JP'])[1 + g % 5],
       now() - g * interval '1 hour'
FROM generate_series(1, 1000) AS g;

INSERT INTO orders (customer_id, total, status, placed_at)
SELECT 1 + g % 1000,
       round((random() * 500)::numeric, 2),
       (ARRAY['new', 'paid', 'shipped', 'cancelled'])[1 + g % 4],
       now() - g * interval '1 minute'
FROM generate_series(1, 20000) AS g;

-- One column per interesting type, for Arrow type-mapping tests.
CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');

CREATE TABLE type_zoo (
    id            serial PRIMARY KEY,
    c_bool        boolean,
    c_int2        smallint,
    c_int4        integer,
    c_int8        bigint,
    c_float4      real,
    c_float8      double precision,
    c_numeric     numeric(20, 6),
    c_numeric_big numeric,
    c_text        text,
    c_varchar     varchar(50),
    c_char        char(5),
    c_bytea       bytea,
    c_date        date,
    c_time        time,
    c_timetz      timetz,
    c_timestamp   timestamp,
    c_timestamptz timestamptz,
    c_interval    interval,
    c_uuid        uuid,
    c_json        json,
    c_jsonb       jsonb,
    c_int_array   integer[],
    c_text_array  text[],
    c_inet        inet,
    c_mood        mood,
    c_point       point,
    c_int4range   int4range
);

INSERT INTO type_zoo (
    c_bool, c_int2, c_int4, c_int8, c_float4, c_float8, c_numeric, c_numeric_big,
    c_text, c_varchar, c_char, c_bytea, c_date, c_time, c_timetz, c_timestamp,
    c_timestamptz, c_interval, c_uuid, c_json, c_jsonb, c_int_array, c_text_array,
    c_inet, c_mood, c_point, c_int4range
) VALUES (
    true, 32767, 2147483647, 9223372036854775807, 3.14, 2.718281828459045,
    12345678901234.123456, 123456789012345678901234567890123456789012.5,
    'hello 🦆', 'varchar', 'abc', '\xdeadbeef', '2026-01-31', '13:37:00', '13:37:00+02',
    '2026-01-31 13:37:00', '2026-01-31 13:37:00+02', '1 day 2 hours',
    'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '{"a": 1}', '{"b": [1, 2]}',
    '{1,2,3}', '{"x","y"}', '192.168.1.1', 'happy', '(1,2)', '[1,10)'
);

INSERT INTO type_zoo DEFAULT VALUES; -- all NULLs

-- 1M rows for grid scrolling and streaming benchmarks.
CREATE TABLE big_table AS
SELECT g AS id,
       md5(g::text) AS hash,
       g % 1000 AS bucket,
       random() * 1000 AS score,
       now() - g * interval '1 second' AS ts
FROM generate_series(1, 1000000) AS g;
ALTER TABLE big_table ADD PRIMARY KEY (id);
