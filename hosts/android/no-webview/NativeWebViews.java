package dev.rusterize;

final class NativeWebViews implements AutoCloseable {
    static final int CAPABILITY = 0;
    NativeWebViews(android.view.View owner,Runnable wake) {}
    byte[] command(int op,java.nio.ByteBuffer data) { throw new UnsupportedOperationException("Build with the webview feature"); }
    void resume() {}
    void suspend() {}
    @Override public void close() {}
}
