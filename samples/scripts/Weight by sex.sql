-- Measurements of tagged animals: males are heavier and longer.
SELECT p.sex,
       count(*) AS measurements,
       round(avg(m.weight_g)) AS avg_weight_g,
       round(avg(m.length_mm)) AS avg_length_mm,
       min(m.weight_g) AS lightest_g,
       max(m.weight_g) AS heaviest_g
FROM measurements m
JOIN platypuses p ON p.id = m.platypus_id
GROUP BY p.sex;
