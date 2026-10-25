-- Sample database for trying PlatypusDB: a platypus field survey. Foreign keys, a CHECK, a
-- view, an index and a few thousand rows, generated deterministically so every build of the
-- file holds the same data. `node scripts/sample-db.mjs` turns it into samples/wildlife.db.

PRAGMA foreign_keys = ON;

CREATE TABLE regions (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE,
    state  TEXT NOT NULL
);

CREATE TABLE rivers (
    id         INTEGER PRIMARY KEY,
    region_id  INTEGER NOT NULL REFERENCES regions (id),
    name       TEXT NOT NULL,
    length_km  REAL NOT NULL
);

CREATE TABLE researchers (
    id           INTEGER PRIMARY KEY,
    full_name    TEXT NOT NULL,
    email        TEXT NOT NULL UNIQUE,
    institution  TEXT NOT NULL,
    joined_on    TEXT NOT NULL
);

CREATE TABLE platypuses (
    id             INTEGER PRIMARY KEY,
    tag_code       TEXT NOT NULL UNIQUE,
    nickname       TEXT,
    sex            TEXT NOT NULL CHECK (sex IN ('F', 'M')),
    born_year      INTEGER,
    home_river_id  INTEGER NOT NULL REFERENCES rivers (id)
);

CREATE TABLE surveys (
    id             INTEGER PRIMARY KEY,
    river_id       INTEGER NOT NULL REFERENCES rivers (id),
    researcher_id  INTEGER NOT NULL REFERENCES researchers (id),
    started_at     TEXT NOT NULL,
    ended_at       TEXT NOT NULL,
    weather        TEXT NOT NULL,
    water_temp_c   REAL
);

CREATE TABLE sightings (
    id           INTEGER PRIMARY KEY,
    survey_id    INTEGER NOT NULL REFERENCES surveys (id),
    platypus_id  INTEGER REFERENCES platypuses (id), -- NULL: an animal without a tag
    seen_at      TEXT NOT NULL,
    behaviour    TEXT NOT NULL CHECK (behaviour IN ('foraging', 'swimming', 'resting', 'burrowing', 'grooming')),
    duration_s   INTEGER NOT NULL,
    notes        TEXT
);
CREATE INDEX sightings_platypus_idx ON sightings (platypus_id);

CREATE TABLE measurements (
    id           INTEGER PRIMARY KEY,
    platypus_id  INTEGER NOT NULL REFERENCES platypuses (id),
    survey_id    INTEGER NOT NULL REFERENCES surveys (id),
    weight_g     INTEGER NOT NULL,
    length_mm    INTEGER NOT NULL,
    bill_mm      REAL NOT NULL
);

CREATE VIEW platypus_stats AS
SELECT p.id, p.tag_code, p.nickname, r.name AS home_river,
       count(s.id) AS sightings,
       min(s.seen_at) AS first_seen,
       max(s.seen_at) AS last_seen
FROM platypuses p
JOIN rivers r ON r.id = p.home_river_id
LEFT JOIN sightings s ON s.platypus_id = p.id
GROUP BY p.id;

INSERT INTO regions (id, name, state) VALUES
    (1, 'Tasmanian Highlands', 'TAS'),
    (2, 'Snowy Mountains', 'NSW'),
    (3, 'Yarra Valley', 'VIC'),
    (4, 'Wet Tropics', 'QLD'),
    (5, 'Blue Mountains', 'NSW'),
    (6, 'Mount Lofty Ranges', 'SA');

INSERT INTO rivers (id, region_id, name, length_km) VALUES
    (1, 1, 'Derwent', 215), (2, 1, 'Mersey', 146), (3, 1, 'Huon', 170),
    (4, 2, 'Snowy', 352), (5, 2, 'Thredbo', 30), (6, 2, 'Eucumbene', 104),
    (7, 3, 'Yarra', 242), (8, 3, 'Watts', 30), (9, 3, 'Little Yarra', 22),
    (10, 4, 'Barron', 165), (11, 4, 'Mossman', 30), (12, 4, 'Daintree', 140),
    (13, 5, 'Grose', 60), (14, 5, 'Coxs', 137), (15, 5, 'Kowmung', 74),
    (16, 6, 'Onkaparinga', 94), (17, 6, 'Torrens', 85), (18, 6, 'Sturt', 30);

WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 12)
INSERT INTO researchers (id, full_name, email, institution, joined_on)
SELECT i,
       first || ' ' || last,
       lower(first) || '.' || lower(last) || '@example.org',
       CASE i % 3 WHEN 0 THEN 'River Ecology Lab' WHEN 1 THEN 'Monotreme Institute' ELSE 'Freshwater Trust' END,
       date('2019-02-01', '+' || (i * 97) || ' days')
FROM n
JOIN (SELECT 1 AS k, 'Alex' AS first UNION ALL SELECT 2, 'Sam' UNION ALL SELECT 3, 'Robin'
      UNION ALL SELECT 4, 'Jordan' UNION ALL SELECT 5, 'Kai' UNION ALL SELECT 6, 'Noor') f ON f.k = 1 + i % 6
JOIN (SELECT 1 AS k, 'Rossi' AS last UNION ALL SELECT 2, 'Nguyen' UNION ALL SELECT 3, 'Walker'
      UNION ALL SELECT 4, 'Okafor' UNION ALL SELECT 5, 'Bianchi' UNION ALL SELECT 6, 'Tanaka'
      UNION ALL SELECT 7, 'Murray') l ON l.k = 1 + i % 7;

WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 300)
INSERT INTO platypuses (id, tag_code, nickname, sex, born_year, home_river_id)
SELECT i,
       printf('PT-%04d', i),
       CASE WHEN i % 4 = 0 THEN NULL ELSE
           (SELECT name FROM (SELECT 0 AS k, 'Bill' AS name UNION ALL SELECT 1, 'Paddle'
            UNION ALL SELECT 2, 'Ripple' UNION ALL SELECT 3, 'Duckie' UNION ALL SELECT 4, 'Murky'
            UNION ALL SELECT 5, 'Webster' UNION ALL SELECT 6, 'Pebble' UNION ALL SELECT 7, 'Splash')
            WHERE k = i % 8) || ' ' || (i / 8 + 1) END,
       CASE i % 2 WHEN 0 THEN 'F' ELSE 'M' END,
       CASE WHEN i % 9 = 0 THEN NULL ELSE 2012 + i % 12 END,
       1 + i % 18
FROM n;

WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000)
INSERT INTO surveys (id, river_id, researcher_id, started_at, ended_at, weather, water_temp_c)
SELECT i,
       1 + (i * 7) % 18,
       1 + i % 12,
       datetime('2023-01-01 05:30', '+' || (i * 13) || ' hours'),
       datetime('2023-01-01 05:30', '+' || (i * 13) || ' hours', '+' || (90 + i % 4 * 30) || ' minutes'),
       CASE i % 5 WHEN 0 THEN 'rain' WHEN 1 THEN 'overcast' WHEN 2 THEN 'clear' WHEN 3 THEN 'fog' ELSE 'wind' END,
       CASE WHEN i % 17 = 0 THEN NULL ELSE round(6 + (i * 37 % 160) / 10.0, 1) END
FROM n;

WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 15000)
INSERT INTO sightings (id, survey_id, platypus_id, seen_at, behaviour, duration_s, notes)
SELECT i,
       sv.id,
       CASE WHEN i % 6 = 0 THEN NULL ELSE 1 + (i * i * 7 + i) % 300 END,
       datetime(sv.started_at, '+' || (i % 90) || ' minutes'),
       CASE i % 5 WHEN 0 THEN 'foraging' WHEN 1 THEN 'swimming' WHEN 2 THEN 'resting'
                  WHEN 3 THEN 'burrowing' ELSE 'grooming' END,
       10 + (i * 29) % 600,
       CASE i % 40 WHEN 0 THEN 'Dived near the bank' WHEN 1 THEN 'Seen with a juvenile'
                   WHEN 2 THEN 'Entered a burrow under tree roots' ELSE NULL END
FROM n
JOIN surveys sv ON sv.id = 1 + (i * 3) % 2000;

WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1200)
INSERT INTO measurements (id, platypus_id, survey_id, weight_g, length_mm, bill_mm)
SELECT i,
       p.id,
       1 + (i * 5) % 2000,
       CASE p.sex WHEN 'M' THEN 1500 ELSE 900 END + (i * 53) % 700,
       CASE p.sex WHEN 'M' THEN 450 ELSE 390 END + (i * 31) % 90,
       round(50 + (i * 19 % 120) / 10.0, 1)
FROM n
JOIN platypuses p ON p.id = 1 + (i * 7) % 300;
