-- Where the platypuses are seen: sightings per river, with their region and how long
-- an animal stayed in view. Ctrl+Enter runs the statement under the cursor.
SELECT rg.name AS region,
       r.name AS river,
       count(s.id) AS sightings,
       round(avg(s.duration_s)) AS avg_seconds
FROM sightings s
JOIN surveys sv ON sv.id = s.survey_id
JOIN rivers r ON r.id = sv.river_id
JOIN regions rg ON rg.id = r.region_id
GROUP BY rg.name, r.name
ORDER BY sightings DESC;
