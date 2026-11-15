# Agent Guidelines

## Syncplay protocol changes

Any change to wire messages, fields, response ordering, heartbeat timing, timeouts, or reconnect behavior must be checked against both the upstream Syncplay client **and server**. Serialization compatibility alone is not sufficient.

- Trace the relevant server behavior in `Syncplay/syncplay`, especially `syncplay/protocols.py` and `syncplay/server.py`.
- Check server-visible side effects on watcher position clocks, connection timeout timestamps, RTT calculations, `ignoringOnTheFly` counters, and forced seek/pause handling.
- Do not assume that omitting an optional field or sending a ping-only/message-only acknowledgement is side-effect free.
- Add regression tests for the affected server-visible behavior, including stalls and reordered or delayed application processing where relevant.
