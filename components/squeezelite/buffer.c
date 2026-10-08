/* 
 *  Squeezelite - lightweight headless squeezebox emulator
 *
 *  (c) Adrian Smith 2012-2015, triode1@btinternet.com
 *      Ralph Irving 2015-2017, ralph_irving@hotmail.com
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 * 
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 */

// fifo bufffers 

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include "squeezelite.h"

// _* called with muxtex locked

inline unsigned _buf_used(struct buffer *buf) {
	return buf->writep >= buf->readp ? buf->writep - buf->readp : buf->size - (buf->readp - buf->writep);
}

unsigned _buf_space(struct buffer *buf) {
	unsigned used = _buf_used(buf);
	if (buf->size <= used + 1) {
		return 0;
	}
	return buf->size - used - 1; // reduce by one as full same as empty otherwise
}

unsigned _buf_cont_read(struct buffer *buf) {
	return buf->writep >= buf->readp ? buf->writep - buf->readp : buf->wrap - buf->readp;
}

unsigned _buf_cont_write(struct buffer *buf) {
	if (buf->writep >= buf->readp) {
		unsigned space = buf->wrap - buf->writep;
		if (buf->readp == buf->buf && space > 0) {
			space--;
		}
		return space;
	} else {
		return (buf->readp > buf->writep) ? (buf->readp - buf->writep - 1) : 0;
	}
}

void _buf_inc_readp(struct buffer *buf, unsigned by) {
	buf->readp += by;
	if (buf->readp >= buf->wrap) {
		buf->readp -= buf->size;
	}
}

void _buf_inc_writep(struct buffer *buf, unsigned by) {
	buf->writep += by;
	if (buf->writep >= buf->wrap) {
		buf->writep -= buf->size;
	}
}

void buf_flush(struct buffer *buf) {
	mutex_lock(buf->mutex);
	buf->readp  = buf->buf;
	buf->writep = buf->buf;
	mutex_unlock(buf->mutex);
}

void _buf_flush(struct buffer *buf) {
	buf->readp  = buf->buf;
	buf->writep = buf->buf;
}

// adjust buffer to multiple of mod bytes so reading in multiple always wraps on frame boundary
void buf_adjust(struct buffer *buf, size_t mod) {
	mutex_lock(buf->mutex);
	buf->base_size = ((size_t)(buf->size / mod)) * mod;
	buf->readp  = buf->writep = buf->buf;
	buf->wrap   = buf->buf + buf->base_size;
	buf->size   = buf->base_size;
	mutex_unlock(buf->mutex);
}

// called with mutex locked to resize, does not retain contents, reverts to original size if fails
void _buf_resize(struct buffer *buf, size_t size) {
	if (size == buf->size) return;
	free(buf->buf);
	buf->buf = malloc(size);
	if (!buf->buf) {
		size    = buf->size;
		buf->buf = malloc(size);
		if (!buf->buf) size = 0;
	}
	buf->writep = buf->readp  = buf->buf;
	buf->wrap   = buf->buf + size;
	buf->true_size = buf->base_size = buf->size = size;
}

size_t _buf_limit(struct buffer *buf, size_t limit) {
	if (limit) {
		buf->size = limit;
		buf->readp = buf->writep = buf->buf;
	} else {
		buf->size = buf->base_size;
	}
	buf->wrap = buf->buf + buf->size;
	return buf->base_size - buf->size;
}

static void _buf_reverse(u8_t *p, size_t len) {
	if (!p || len < 2) return;
	u8_t *q = p + len - 1;
	while (p < q) {
		u8_t t = *p;
		*p++ = *q;
		*q-- = t;
	}
}

void _buf_unwrap(struct buffer *buf, size_t cont) {
	ssize_t by = cont - (buf->wrap - buf->readp);

	// do nothing if we have enough space
	if (by <= 0 || cont >= buf->size) return;

	// buffer already unwrapped, just move it up
	if (buf->writep >= buf->readp) {
		memmove(buf->readp - by, buf->readp, buf->writep - buf->readp);
		buf->readp -= by;
		buf->writep -= by;
		return;
	}

	// MEM-01: In-place 3-reversal rotation algorithm:
	// reverse(A), reverse(B), reverse(AB) rotates buf by 'by' bytes to the left.
	// This makes cont contiguous bytes available at readp without dynamic malloc
	// or recursion on malloc failure in the audio path.
	_buf_reverse(buf->buf, by);
	_buf_reverse(buf->buf + by, buf->size - by);
	_buf_reverse(buf->buf, buf->size);

	buf->readp -= by;
	if (buf->writep >= buf->buf + by) {
		buf->writep -= by;
	} else {
		buf->writep += buf->size - by;
	}
	if (buf->writep >= buf->wrap) {
		buf->writep -= buf->size;
	}
}

void buf_init(struct buffer *buf, size_t size) {
	buf->buf    = malloc(size);
	buf->readp  = buf->buf;
	buf->writep = buf->buf;
	buf->wrap   = buf->buf + size;
	buf->true_size = buf->base_size = buf->size = size;
	mutex_create_p(buf->mutex);
}

void buf_destroy(struct buffer *buf) {
	if (buf->buf) {
		free(buf->buf);
		buf->buf = NULL;
		buf->size = buf->base_size = buf->true_size = 0;
		mutex_destroy(buf->mutex);
	}
}
