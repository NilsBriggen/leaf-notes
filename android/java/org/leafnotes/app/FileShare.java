package org.leafnotes.app;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileNotFoundException;
import java.net.URLConnection;

/**
 * Lets another app read one exported file (read-only, by a link Leaf Notes hands it) on Android 8
 * and 9, which have no shared Downloads folder an app can write to without asking. The camera app
 * may also write the photo being taken ("photo-…" files) there.
 */
public class FileShare extends ContentProvider {
    static final String AUTHORITY = "org.leafnotes.app.files";

    static File dir(android.content.Context c) { File d = new File(c.getCacheDir(), "shared"); d.mkdirs(); return d; }

    static Uri uri(File f) { return new Uri.Builder().scheme("content").authority(AUTHORITY).appendPath(f.getName()).build(); }

    /** Copies handed out more than a day ago aren't needed any more. */
    static void tidy(android.content.Context c) {
        File[] old = dir(c).listFiles();
        if (old != null) for (File f : old) if (System.currentTimeMillis() - f.lastModified() > 86_400_000L) f.delete();
    }

    static File file(android.content.Context c, Uri uri) throws FileNotFoundException {
        String name = uri.getLastPathSegment();
        if (name == null || name.contains("/") || name.startsWith(".")) throw new FileNotFoundException();
        File f = new File(dir(c), name);
        if (!f.isFile()) throw new FileNotFoundException();
        return f;
    }

    private File file(Uri uri) throws FileNotFoundException { return file(getContext(), uri); }

    @Override public boolean onCreate() { return true; }

    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        File f = file(uri);
        // only the camera's photo can be written, everything else is read-only
        boolean write = mode.contains("w") && f.getName().startsWith("photo-");
        return ParcelFileDescriptor.open(f, write ? ParcelFileDescriptor.parseMode(mode) : ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override public String getType(Uri uri) {
        String t = URLConnection.guessContentTypeFromName(uri.getLastPathSegment());
        return t != null ? t : "application/octet-stream";
    }

    /** Name and size, which share targets show and some need. */
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String sort) {
        MatrixCursor c = new MatrixCursor(new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE});
        try { File f = file(uri); c.addRow(new Object[]{f.getName(), f.length()}); } catch (FileNotFoundException e) { /* nothing to show */ }
        return c;
    }

    @Override public Uri insert(Uri uri, ContentValues values) { return null; }
    @Override public int delete(Uri uri, String selection, String[] args) { return 0; }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { return 0; }
}
