-- The session and lock lists of the admin tab read other sessions' transactions and locks:
-- InnoDB's tables need PROCESS, MySQL 8's lock tables `performance_schema`.
GRANT PROCESS ON *.* TO 'platypus'@'%';
GRANT SELECT ON performance_schema.* TO 'platypus'@'%';
