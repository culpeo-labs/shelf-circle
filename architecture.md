# Leecommend — High-Level Architecture

## Core idea
A small, friends-first social layer around reading: what you're reading, what you've finished, and direct recommendations to specific friends — not a public review feed. This is a non-commercial, contribute-to-the-world project, not a business. The primary path from a book page is **"find at your local library"**; a small affiliate link (Bookshop.org, Libro.fm) is a secondary, optional option that exists only to help cover hosting costs, not to generate income.

## Core entities (data model sketch)
- **User** — id, display name, handle, avatar, joined_at
- **Friendship / Follow** — simple mutual "friends" model to start (not one-way follow), since the product is close-circle by design
- **Book** — canonical record: title, authors, cover image, language, ISBN(s)/edition ids, external source ids (Open Library work/edition id, Google Books id)
- **BookStatus** (per user, per book) — want_to_read / currently_reading / finished / did_not_finish, timestamps, optional progress %
- **Recommendation** — from_user, to_user (or to a group of friends), book, note/comment, created_at
- **Reaction/Comment** — lightweight, on a status update or recommendation (keep this minimal — not trying to be a review site)
- **LibraryAvailability** — book_id, user's approximate location/library system, availability status, deep link into Libby/OverDrive (or WorldCat search fallback) — this is the primary "get this book" action
- **AffiliateLink** (secondary/optional) — book_id, retailer (Bookshop.org / Libro.fm), url, region/language variant — shown only as a smaller "or buy a copy" option beneath the library option

## Multi-language / multi-source book data
Single source (Open Library or Google Books alone) won't cover non-English catalogs well. Plan for a **book resolution layer**:
1. Query Open Library first (broad, free, decent international coverage)
2. Fall back to / merge with Google Books API for gaps and richer metadata
3. Normalize into your own `Book` table with a canonical id, so status/recommendations attach to *your* record, not a vendor id
4. Store multiple editions/translations under one logical "work" so a friend recommending the Spanish edition and you reading the English one still link up

## High-level components
1. **Mobile client** (iOS/Android — likely React Native or Flutter for one codebase, given small team of you + friends)
2. **API backend** — auth, friend graph, book status, recommendations feed
3. **Book data service** — wraps Open Library + Google Books, caches results, handles merging/normalization
4. **Library lookup service (primary "get this book" path)** — WorldCat API for "held near you" lookups as a lightweight v1; longer-term, reach out to OverDrive for their Library Link / Libby deep-linking integration (not self-serve — requires direct partnership contact)
5. **Affiliate link resolver (secondary, optional)** — given a canonical book + user locale, returns a Bookshop.org/Libro.fm link only as a fallback "or buy a copy" option beneath the library result; not shown as the default action
6. **Feed/notifications** — friend activity feed, "X recommended a book to you" pushes
7. **Database** — Postgres is a natural fit (relational: users, books, statuses, recommendations all relate cleanly)

## Suggested starting stack (lightweight, small-team friendly)
- Backend: a single API service (Node/TypeScript or your preferred language) + Postgres
- Book data: Open Library API (no key needed) + Google Books API (free tier, key required) behind your own caching layer
- Mobile: React Native (fastest path to iOS + Android with a small team)
- Auth: something managed (e.g., Supabase/Auth0/Firebase Auth) to avoid building this yourselves for v1
- Hosting: whatever you're already comfortable with — this app's load will be trivial at friends-only scale

## Suggested build order (v1, friends-only)
1. Book search/lookup (resolve + normalize from Open Library/Google Books)
2. User accounts + friend graph
3. Reading status (want to read / reading / finished)
4. Recommendations between friends
5. Library availability lookup on book pages (primary action — WorldCat API for v1)
6. Affiliate link attachment on book pages, shown as a secondary/optional action beneath the library option
7. Basic feed of friend activity

## Open questions to settle before building
- Friend model: mutual friends only, or also small private groups (book clubs of 3-5)?
- How much "review" content, if any — just a status + short note, or full-length reviews?
- Growth path: if this grows past friends, does the friend-first design survive, or does it need a public/discovery mode later?
- Non-commercial framing: worth stating explicitly somewhere (README, about page) that this isn't a business — affiliate links exist only to offset hosting, nothing more