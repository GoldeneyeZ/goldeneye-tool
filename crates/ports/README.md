# Ports

This layer will contain narrow interfaces owned by application use cases.

Ports may depend only on `goldeneye-domain` and other ports. Add a port only
while removing a concrete application-to-adapter dependency; unused or
speculative interfaces are forbidden.
