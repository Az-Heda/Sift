# Sift
find what matters, remove what doesn't.

This is a brief description of the project, for the features read down below:

what's required is the users will add some folders to this app, it automatically scans every file (maybe only images?) in those folders, and tells you the duplicates.


---

## 1. Features I'd add

I'd organize the product into six main features.

### A. Smart scanning

- Add multiple folders.
- Scan subfolders recursively.
- Support images initially: JPG, PNG, WEBP, GIF, TIFF, HEIC.
- Later support videos, PDFs, audio, archives and other files.
- Automatically detect newly added or modified files.
- Allow manual rescanning.
- Exclude folders such as system directories, caches and temporary files.

### B. Three levels of duplicate detection

This is probably the most important technical/product decision.

| Detection               | What it finds                                 | Priority |
| ----------------------- | --------------------------------------------- | -------- |
| Exact duplicates        | Identical files, even with different names    | MVP      |
| Visually similar images | Resized, compressed or slightly edited images | V2       |
| Near-duplicates         | Similar photos, burst shots, alternate edits  | V3       |

For example:

- `IMG_001.jpg`
- `vacation-final.jpg`
- `copy-of-vacation.jpg`

Even if their filenames and locations differ, the app can detect that they're identical.

For exact duplicates, use file hashes such as SHA-256. For similar images, use perceptual hashes (pHash).

### C. A proper duplicate review interface

Don't just show a list of filenames.

Show:

- Image previews.
- File size.
- File location.
- Date created and modified.
- Resolution.
- Number of copies.
- Which copy is recommended to keep.

Allow users to:

- Select individual files.
- Select all duplicates except the recommended original.
- Compare two images side by side.
- Open the containing folder.
- Reveal files in the operating system.

### D. Smart cleanup

This is where the app becomes genuinely useful.

- Delete selected duplicates.
- Move duplicates to a quarantine folder instead of immediately deleting them.
- Restore quarantined files.
- Keep the highest-resolution copy.
- Prefer the oldest or newest copy.
- Prefer a copy in a designated master folder.
- Never automatically delete without user approval.

### E. Filters and organization

- Filter by file type.
- Filter by file size.
- Filter by folder.
- Sort by largest space savings.
- Sort by number of copies.
- Show duplicates across different folders only.
- Search by filename.

### F. Statistics and insights

A dashboard could show:

- Total files scanned.
- Total storage scanned.
- Duplicate groups found.
- Wasted storage.
- Storage recovered.
- Most duplicated folders.
- Most common duplicate file types.


## 2. Features that could make it stand out

These are the features I'd consider after the core product works.

How to Turbocharge Your Photography Workflow | PetaPixel

Visual similarity search

Find photos that look alike even when they aren't byte-for-byte identical. Great for identifying edited, resized or compressed images.

High-value feature

PIXLPath: Digital asset management for Apple the ecosystem

Best photo selection

When there are five nearly identical photos, suggest the sharpest, highest-resolution version, or one without blur.

Dribbble Dashboard Designs at Esther Corbett blog

Storage intelligence

Find unusually large files, old files, empty folders and folders consuming the most space.

Sortwise - Automated media manager

Folder organization assistant

Suggest ways to organize files by date, file type, project or other criteria. Let users preview proposed changes before applying them.


## 3. How I'd build it technically

One important decision: I'd make this a desktop application, not a normal website.

A browser-based app has restrictions when accessing arbitrary folders on someone's computer. A desktop app makes folder selection, recursive scanning, file watching and cleanup much more straightforward.

My recommended stack:

Frontend

React + TypeScript

Beautiful interface, folder management, image previews, duplicate comparison and dashboard.Desktop runtime

Tauri + Rust

Native filesystem access, background scanning, hashing, file operations and filesystem watching.Local database

SQLite

Store scanned files, hashes, metadata, duplicate groups and scan history.Image processing

Image libraries

Extract image metadata, generate thumbnails and calculate perceptual hashes.


### The scanning architecture

I'd build the scanning engine as a pipeline:


1. Folder selection

User adds one or more folders


2. Recursive file discovery

Find files and read metadata


3. Group by file size

Skip files that cannot possibly match


4. Calculate SHA-256

Compare content of potential duplicates


5. Group duplicates

Create groups of files with matching hashes


6. Display results

Review, compare and safely clean up

A few technical details worth getting right:

- Cache hashes in SQLite so rescanning doesn't mean hashing every file again.
- Use file path, size and modification time to determine whether a cached hash is still valid.
- Run scans in background threads and expose progress to the UI.
- Support cancellation and resuming.
- Handle inaccessible folders, symlinks and files changing during scans.
- Never load entire large files into memory just to hash them.
- Keep all scanning and image analysis local. No cloud upload required.

For image similarity, I'd add a separate pipeline using perceptual hashing, not mix it with exact duplicate detection.

## 4. Suggested development roadmap

| Version  | Features                                                                 |
| -------- | ------------------------------------------------------------------------ |
| V1 — MVP | Folder selection, recursive scanning, exact duplicate detection, results |
| V1.1     | Image previews, filters, sorting, file comparison                        |
| V1.2     | Safe deletion, quarantine, restore                                       |
| V2       | Similar image detection, perceptual hashes                               |
| V2.1     | Videos, PDFs, audio and other file types                                 |
| V3       | Automatic monitoring, storage analytics, smart recommendations           |

## 5. What I'd actually build first

I'd resist the temptation to build everything immediately.

The first release should have just five screens:


1. Dashboard — folders, scan statistics and potential space savings.
2. Folders — add/remove folders and see scan status.
3. Duplicates — grouped duplicate files with previews.
4. Compare — inspect copies and choose which to keep.
5. Settings — excluded folders, file types and scanning preferences.

And I'd make these three product decisions:

- Local-first: No accounts, no cloud, no uploading personal files.
- Safety-first: Never delete anything automatically. Quarantine before permanent deletion.
- Fast: Incremental scanning, so subsequent scans are significantly quicker.

## 6. My suggested product identity

I'd position this as more than a duplicate finder.

* Sift — find what matters, remove what doesn't.

## 8. Visual identity

The project must have a futuristic looks and colors, but with a fully customizable theme, with both light/dark theme defaulting on the system