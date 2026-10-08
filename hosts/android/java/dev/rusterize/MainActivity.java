package dev.rusterize;

import android.app.Activity;
import android.os.Bundle;
import android.view.WindowInsets;
import android.view.View;
import android.widget.FrameLayout;

public final class MainActivity extends Activity {
    private RusterizeView view;
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(0xff0c121c);
        root.setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
          @Override public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets safe = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
                v.setPadding(safe.left, safe.top, safe.right, safe.bottom);
            } else {
                v.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(), insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            }
            return insets;
          }
        });
        view = new RusterizeView(this);
        root.addView(view, new FrameLayout.LayoutParams(-1, -1));
        setContentView(root);
        view.requestFocus();
    }
    @Override protected void onResume() { super.onResume(); if (view != null) view.resume(); }
    @Override protected void onPause() { if (view != null) view.suspend(); super.onPause(); }
    @Override protected void onDestroy() { if (view != null) view.close(); super.onDestroy(); }
}
