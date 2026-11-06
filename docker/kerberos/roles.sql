-- The role of the Kerberos principal alice@PLATYPUS.TEST. It has no password.
CREATE ROLE alice LOGIN;
GRANT CONNECT ON DATABASE platypus TO alice;
