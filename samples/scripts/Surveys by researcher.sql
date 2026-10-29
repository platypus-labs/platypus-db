-- Who went out on the rivers, and when: a LEFT JOIN keeps the researchers with no survey.
SELECT rs.full_name,
       rs.institution,
       count(sv.id) AS surveys,
       min(sv.started_at) AS first_survey,
       max(sv.started_at) AS last_survey
FROM researchers rs
LEFT JOIN surveys sv ON sv.researcher_id = rs.id
GROUP BY rs.id
ORDER BY surveys DESC;
