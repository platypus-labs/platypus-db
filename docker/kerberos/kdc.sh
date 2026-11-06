#!/bin/sh
# A Kerberos server for the realm PLATYPUS.TEST, test-only. It makes the principals once:
#   postgres/postgres-gss.platypus.test  the PostgreSQL service
#   alice                                a user, who gets her ticket from a keytab
# and leaves their keytabs in /keytabs, a volume the server and the client share.
set -eu

if [ ! -f /var/lib/krb5kdc/principal ]; then
  kdb5_util create -s -r PLATYPUS.TEST -P test-only-master-key
fi
kadmin.local -q "addprinc -randkey postgres/postgres-gss.platypus.test@PLATYPUS.TEST"
kadmin.local -q "addprinc -randkey alice@PLATYPUS.TEST"
rm -f /keytabs/postgres.keytab /keytabs/alice.keytab /keytabs/.ready
kadmin.local -q "ktadd -k /keytabs/postgres.keytab postgres/postgres-gss.platypus.test@PLATYPUS.TEST"
kadmin.local -q "ktadd -k /keytabs/alice.keytab alice@PLATYPUS.TEST"
chmod 644 /keytabs/postgres.keytab /keytabs/alice.keytab
touch /keytabs/.ready
exec krb5kdc -n
