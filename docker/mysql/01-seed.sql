-- Demo schema for PlatypusDB development and integration tests (MySQL 8.4 and MariaDB 11).

USE platypus;

CREATE TABLE customers (
    id          BIGINT AUTO_INCREMENT PRIMARY KEY,
    email       VARCHAR(255) NOT NULL UNIQUE,
    full_name   VARCHAR(255) NOT NULL,
    country     CHAR(2),
    created_at  DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
);

CREATE TABLE orders (
    id           BIGINT AUTO_INCREMENT PRIMARY KEY,
    customer_id  BIGINT NOT NULL,
    total        DECIMAL(12, 2) NOT NULL,
    status       ENUM('new', 'paid', 'shipped', 'cancelled') NOT NULL,
    placed_at    DATETIME NOT NULL,
    INDEX orders_customer_id_idx (customer_id),
    FOREIGN KEY (customer_id) REFERENCES customers (id)
);

CREATE VIEW customer_totals AS
SELECT c.id, c.full_name, COUNT(o.id) AS orders, COALESCE(SUM(o.total), 0) AS spent
FROM customers c
LEFT JOIN orders o ON o.customer_id = c.id
GROUP BY c.id, c.full_name;

-- Digits 0..9, cross-joined to generate row series without recursion limits.
CREATE TABLE digits (d TINYINT PRIMARY KEY);
INSERT INTO digits VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9);

INSERT INTO customers (email, full_name, country, created_at)
SELECT CONCAT('user', n, '@example.com'),
       CONCAT('Customer ', n),
       ELT(1 + n % 5, 'IT', 'DE', 'FR', 'US', 'JP'),
       NOW() - INTERVAL n HOUR
FROM (SELECT 1 + a.d + 10 * b.d + 100 * c.d AS n
      FROM digits a, digits b, digits c) s;

INSERT INTO orders (customer_id, total, status, placed_at)
SELECT 1 + n % 1000,
       ROUND(RAND() * 500, 2),
       ELT(1 + n % 4, 'new', 'paid', 'shipped', 'cancelled'),
       NOW() - INTERVAL n MINUTE
FROM (SELECT 1 + a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 10000 * f.d AS n
      FROM digits a, digits b, digits c, digits e, digits f) s
WHERE n <= 20000;

-- One column per interesting type, for Arrow type-mapping tests.
CREATE TABLE type_zoo (
    id           INT AUTO_INCREMENT PRIMARY KEY,
    c_bool       BOOLEAN,
    c_tinyint    TINYINT,
    c_smallint   SMALLINT,
    c_int        INT,
    c_bigint     BIGINT,
    c_ubigint    BIGINT UNSIGNED,
    c_float      FLOAT,
    c_double     DOUBLE,
    c_decimal    DECIMAL(20, 6),
    c_varchar    VARCHAR(50),
    c_text       TEXT,
    c_char       CHAR(5),
    c_blob       BLOB,
    c_binary     VARBINARY(16),
    c_date       DATE,
    c_time       TIME(6),
    c_datetime   DATETIME(6),
    c_timestamp  TIMESTAMP NULL,
    c_year       YEAR,
    c_json       JSON,
    c_enum       ENUM('sad', 'ok', 'happy'),
    c_set        SET('a', 'b', 'c'),
    c_bit        BIT(8)
) DEFAULT CHARSET = utf8mb4;

INSERT INTO type_zoo (
    c_bool, c_tinyint, c_smallint, c_int, c_bigint, c_ubigint, c_float, c_double, c_decimal,
    c_varchar, c_text, c_char, c_blob, c_binary, c_date, c_time, c_datetime, c_timestamp,
    c_year, c_json, c_enum, c_set, c_bit
) VALUES (
    TRUE, 127, 32767, 2147483647, 9223372036854775807, 18446744073709551615, 3.14,
    2.718281828459045, 12345678901234.123456, 'varchar', 'hello', 'abc', X'DEADBEEF',
    X'00FF', '2026-01-31', '13:37:00.123456', '2026-01-31 13:37:00.123456',
    '2026-01-31 13:37:00', 2026, '{"a": 1}', 'happy', 'a,c', b'10101010'
);

INSERT INTO type_zoo () VALUES (); -- all NULLs

-- 1M rows for grid scrolling and streaming benchmarks.
CREATE TABLE big_table (
    id      INT PRIMARY KEY,
    hash    CHAR(32) NOT NULL,
    bucket  INT NOT NULL,
    score   DOUBLE NOT NULL,
    ts      DATETIME NOT NULL
);

INSERT INTO big_table
SELECT n, MD5(n), n % 1000, RAND() * 1000, NOW() - INTERVAL n SECOND
FROM (SELECT 1 + a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 10000 * f.d + 100000 * g.d AS n
      FROM digits a, digits b, digits c, digits e, digits f, digits g) s;
