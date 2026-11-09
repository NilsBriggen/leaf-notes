package org.leafnotes.app;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.MediaStore;
import android.provider.OpenableColumns;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import android.widget.Toast;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.security.KeyStore;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ConcurrentLinkedQueue;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/**
 * The few things a native app can't do on Android without Java: the system file picker (its
 * answer comes back to an Activity method), files shared into the app, opening attachments in
 * other apps, the hardware-backed key store and the system language. Everything else is Rust.
 */
public class LeafActivity extends NativeActivity {
    private static final int PICK = 4200;
    private static final int CAMERA = 4400;
    /** Where the camera app writes the photo being taken (Pictures/Leaf Notes, so it's in the gallery too). */
    private Uri photo;
    /** Files that arrived (picked or shared), as "kind|path", collected by the Rust side. */
    private final ConcurrentLinkedQueue<String> incoming = new ConcurrentLinkedQueue<>();

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        takeShared(getIntent());
    }

    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        takeShared(intent);
    }

    /** Show the system file picker; kind is passed back with the result. */
    public void pickFile(final int kind, final String mime, final boolean many) {
        runOnUiThread(new Runnable() { public void run() {
            Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType(mime);
            if (mime.contains(",")) { i.setType("*/*"); i.putExtra(Intent.EXTRA_MIME_TYPES, mime.split(",")); }
            i.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, many);
            try { startActivityForResult(i, PICK + kind); }
            catch (ActivityNotFoundException e) { toast("No file picker is available on this device."); }
        }});
    }

    /** Take a photo with the camera app; it arrives like a picked picture (kind 1). */
    public void takePhoto() {
        runOnUiThread(new Runnable() { public void run() {
            try {
                ContentValues v = new ContentValues();
                v.put(MediaStore.MediaColumns.DISPLAY_NAME, "Leaf Notes " + System.currentTimeMillis() + ".jpg");
                v.put(MediaStore.MediaColumns.MIME_TYPE, "image/jpeg");
                if (Build.VERSION.SDK_INT >= 29) v.put(MediaStore.MediaColumns.RELATIVE_PATH, "Pictures/Leaf Notes");
                photo = getContentResolver().insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, v);
                Intent i = new Intent(MediaStore.ACTION_IMAGE_CAPTURE).putExtra(MediaStore.EXTRA_OUTPUT, photo)
                    .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_GRANT_READ_URI_PERMISSION);
                startActivityForResult(i, CAMERA);
            } catch (Exception e) { toast("No camera app is available."); }
        }});
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == CAMERA && photo != null) {
            final Uri u = photo;
            photo = null;
            if (result != RESULT_OK) { try { getContentResolver().delete(u, null, null); } catch (Exception e) { /* nothing to tidy */ } return; }
            new Thread(new Runnable() { public void run() { String p = copyIn(u); if (p != null) incoming.add("1|" + p); } }).start();
            return;
        }
        if (request < PICK || request > PICK + 99 || result != RESULT_OK || data == null) return;
        final int kind = request - PICK;
        final List<Uri> uris = new ArrayList<>();
        if (data.getClipData() != null) for (int k = 0; k < data.getClipData().getItemCount(); k++) uris.add(data.getClipData().getItemAt(k).getUri());
        else if (data.getData() != null) uris.add(data.getData());
        new Thread(new Runnable() { public void run() { for (Uri u : uris) { String p = copyIn(u); if (p != null) incoming.add(kind + "|" + p); } } }).start();
    }

    /** "Share to Leaf Notes" from other apps: files and text. */
    private void takeShared(Intent it) {
        if (it == null) return;
        String action = it.getAction();
        if (Intent.ACTION_SEND.equals(action)) {
            Uri u = it.getParcelableExtra(Intent.EXTRA_STREAM);
            String text = it.getStringExtra(Intent.EXTRA_TEXT);
            if (u != null) new Thread(new Runnable() { public void run() { String p = copyIn(u); if (p != null) incoming.add("0|" + p); } }).start();
            else if (text != null) {
                try {
                    File f = new File(inbox(), System.nanoTime() + "-shared.txt");
                    try (OutputStream o = new FileOutputStream(f)) { o.write(text.getBytes("UTF-8")); }
                    incoming.add("9|" + f.getAbsolutePath());
                } catch (Exception e) { /* nothing arrives */ }
            }
        } else if (Intent.ACTION_SEND_MULTIPLE.equals(action)) {
            final ArrayList<Uri> us = it.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
            if (us != null) new Thread(new Runnable() { public void run() { for (Uri u : us) { String p = copyIn(u); if (p != null) incoming.add("0|" + p); } } }).start();
        }
        it.setAction(null); // handled once
    }

    /** Something arrived (the app is woken to collect it). */
    public boolean hasIncoming() { return !incoming.isEmpty(); }

    /** Everything that arrived since the last call, one "kind|path" per line. */
    public String takeIncoming() {
        StringBuilder b = new StringBuilder();
        for (String s; (s = incoming.poll()) != null; ) b.append(s).append('\n');
        return b.toString();
    }

    private File inbox() { File d = new File(getCacheDir(), "incoming"); d.mkdirs(); return d; }

    /** Copy a picked or shared document into the app (keeping its name). */
    private String copyIn(Uri uri) {
        String name = "file";
        try (Cursor c = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst() && c.getString(0) != null) name = c.getString(0);
        } catch (Exception e) { /* keep the default name */ }
        name = name.replaceAll("[^A-Za-z0-9._ -]", "_");
        // a name without an extension (some apps share "image"): add one from the file's type, so a photo arrives as a picture
        if (!name.contains(".")) {
            String type = getContentResolver().getType(uri), ext = type == null ? null : android.webkit.MimeTypeMap.getSingleton().getExtensionFromMimeType(type);
            if (ext != null) name = name + "." + ext;
        }
        File f = new File(inbox(), System.nanoTime() + "-" + name);
        try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(f)) {
            if (in == null) return null;
            byte[] buf = new byte[65536];
            for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
            return f.getAbsolutePath();
        } catch (Exception e) { return null; }
    }

    /** A copy of the file in Downloads/Leaf Notes, which other apps may read (null if that failed).
     *  Android 8 and 9: a copy other apps may read through FileShare instead. */
    private Uri toDownloads(String path, String name, String mime) throws Exception {
        if (Build.VERSION.SDK_INT < 29) {
            File f = new File(FileShare.dir(this), name.replaceAll("[/\\\\]", "_"));
            try (InputStream in = new FileInputStream(path); OutputStream out = new FileOutputStream(f)) {
                byte[] buf = new byte[65536];
                for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
            }
            return new Uri.Builder().scheme("content").authority(FileShare.AUTHORITY).appendPath(f.getName()).build();
        }
        ContentResolver r = getContentResolver();
        ContentValues v = new ContentValues();
        v.put(MediaStore.MediaColumns.DISPLAY_NAME, name);
        v.put(MediaStore.MediaColumns.MIME_TYPE, mime);
        v.put(MediaStore.MediaColumns.RELATIVE_PATH, "Download/Leaf Notes");
        Uri u = r.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v);
        if (u == null) { toast("Couldn't copy the file to Downloads."); return null; }
        try (InputStream in = new FileInputStream(path); OutputStream out = r.openOutputStream(u)) {
            byte[] buf = new byte[65536];
            for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
        }
        return u;
    }

    /** Open an attachment in another app. */
    public void openFile(final String path, final String name, final String mime) {
        runOnUiThread(new Runnable() { public void run() {
            try {
                Uri u = toDownloads(path, name, mime);
                if (u == null) return;
                Intent view = new Intent(Intent.ACTION_VIEW).setDataAndType(u, mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                try { startActivity(view); } catch (ActivityNotFoundException e) { toast("No app here can open this file. It's in Downloads/Leaf Notes."); }
            } catch (Exception e) { toast("Couldn't open the file: " + e.getMessage()); }
        }});
    }

    /** Hand an exported file to the share menu (mail, Drive, Files…); a copy also stays in Downloads/Leaf Notes. */
    public void shareFile(final String path, final String name, final String mime) {
        runOnUiThread(new Runnable() { public void run() {
            try {
                Uri u = toDownloads(path, name, mime);
                if (u == null) return;
                Intent send = new Intent(Intent.ACTION_SEND).setType(mime).putExtra(Intent.EXTRA_STREAM, u).putExtra(Intent.EXTRA_TITLE, name).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                send.setClipData(ClipData.newUri(getContentResolver(), name, u)); // shows the file's name in the share sheet
                Intent chooser = Intent.createChooser(send, name);
                // the share sheet itself reads the file's name for its preview
                chooser.setClipData(send.getClipData());
                chooser.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                // Leaf Notes itself accepts shared files; don't offer it as a place to send its own export
                chooser.putExtra(Intent.EXTRA_EXCLUDE_COMPONENTS, new android.content.ComponentName[] { new android.content.ComponentName(LeafActivity.this, LeafActivity.class) });
                startActivity(chooser);
            } catch (Exception e) { toast("Couldn't share the file: " + e.getMessage()); }
        }});
    }

    private void toast(final String s) { runOnUiThread(new Runnable() { public void run() { Toast.makeText(LeafActivity.this, s, Toast.LENGTH_LONG).show(); } }); }

    /** Ask for the microphone (from the main thread, where Android shows its dialog reliably). */
    public void requestMic() {
        runOnUiThread(new Runnable() { public void run() { requestPermissions(new String[]{android.Manifest.permission.RECORD_AUDIO}, 7); } });
    }

    /** A recording started or stopped: hold the app in the foreground meanwhile (see RecordService). */
    public void recording(final boolean on) {
        runOnUiThread(new Runnable() { public void run() {
            Intent i = new Intent(LeafActivity.this, RecordService.class);
            try { if (on) startForegroundService(i); else stopService(i); }
            catch (Exception e) { if (on) toast("Recording may stop when the screen turns off."); }
        }});
    }

    /** The system language ("de", "fr", "en" …). */
    public String language() { return Locale.getDefault().getLanguage(); }

    // ---- secrets: AES-GCM under a key that lives in the Android key store and never leaves it ----

    private SecretKey key() throws Exception {
        KeyStore ks = KeyStore.getInstance("AndroidKeyStore");
        ks.load(null);
        if (ks.containsAlias("leafnotes-secrets")) return ((KeyStore.SecretKeyEntry) ks.getEntry("leafnotes-secrets", null)).getSecretKey();
        KeyGenerator g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        g.init(new KeyGenParameterSpec.Builder("leafnotes-secrets", KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).setKeySize(256).build());
        return g.generateKey();
    }

    /** Encrypt a secret; "" if the key store isn't available (the caller then keeps it as before). */
    public String protect(String plain) {
        try {
            Cipher c = Cipher.getInstance("AES/GCM/NoPadding");
            c.init(Cipher.ENCRYPT_MODE, key());
            byte[] iv = c.getIV(), data = c.doFinal(plain.getBytes("UTF-8"));
            byte[] out = new byte[iv.length + data.length];
            System.arraycopy(iv, 0, out, 0, iv.length);
            System.arraycopy(data, 0, out, iv.length, data.length);
            return Base64.encodeToString(out, Base64.NO_WRAP);
        } catch (Exception e) { return ""; }
    }

    /** Decrypt what protect() made; "" if it can't be (e.g. the app was reinstalled). */
    public String unprotect(String b64) {
        try {
            byte[] all = Base64.decode(b64, Base64.NO_WRAP);
            Cipher c = Cipher.getInstance("AES/GCM/NoPadding");
            c.init(Cipher.DECRYPT_MODE, key(), new GCMParameterSpec(128, all, 0, 12));
            return new String(c.doFinal(all, 12, all.length - 12), "UTF-8");
        } catch (Exception e) { return ""; }
    }
}
