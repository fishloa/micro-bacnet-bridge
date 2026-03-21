/* Minimal stdlib.h stub for freestanding ARM builds.
 * Only declares functions referenced by bacnet-stack mstp.c. */
#ifndef _STDLIB_H_STUB
#define _STDLIB_H_STUB

int rand(void);
void srand(unsigned int seed);

#endif
