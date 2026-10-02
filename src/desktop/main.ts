import { readEmbedded } from 'perry';
import { alert, App, WebView } from 'perry/ui';

const html = readEmbedded('dist/desktop/index.html');

// ponytail: data URL has no persistent origin; use a local origin if storage is needed.
App({
    title: 'Drip',
    width: 1024,
    height: 768,
    body: WebView({
        url: 'data:text/html;base64,' + html.toString('base64'),
        width: 1024,
        height: 768,
        onError: (code: number, message: string) => alert('Drip could not load', `WebView2 error ${code}: ${message}`),
    }),
});
