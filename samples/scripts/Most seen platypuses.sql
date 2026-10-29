-- The animals seen most often, from the view platypus_stats.
SELECT tag_code, nickname, home_river, sightings, first_seen, last_seen
FROM platypus_stats
ORDER BY sightings DESC
LIMIT 20;
