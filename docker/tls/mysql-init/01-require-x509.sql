-- The server rejects a login without a client certificate signed by the test CA.
ALTER USER 'platypus'@'%' REQUIRE X509;
