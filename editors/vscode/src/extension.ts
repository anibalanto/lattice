import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import * as fs from 'fs';
import { execSync, spawn } from 'child_process';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: vscode.ExtensionContext) {
    const serverBin = findBinary('bilinker-lsp');
    if (!serverBin) {
        vscode.window.showErrorMessage(
            'bilinker-lsp not found. Run: cargo install --path crates/bilinker-lsp'
        );
        return;
    }

    const serverOptions: ServerOptions = {
        command: serverBin,
        transport: TransportKind.stdio,
    };

    const clientOptions: LanguageClientOptions = {
        // Activate for all file types — bilinks can reference any file
        documentSelector: [{ scheme: 'file', pattern: '**/*' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/.bilink/**'),
        },
    };

    client = new LanguageClient('bilinker', 'Bilinker', serverOptions, clientOptions);
    client.start();

    // Command: open graph for current file
    context.subscriptions.push(
        vscode.commands.registerCommand('bilinker.openGraph', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) return;
            openGraph(editor.document.uri.fsPath, false);
        })
    );

    // Command: open full system graph
    context.subscriptions.push(
        vscode.commands.registerCommand('bilinker.openSystemGraph', () => {
            const ws = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
            if (!ws) return;
            openGraph(ws, true);
        })
    );

    // Handle code lens click to show bilinks in a panel
    context.subscriptions.push(
        vscode.commands.registerCommand('bilinker.showBilinks', async (uri: string, ids: string[]) => {
            const panel = vscode.window.createWebviewPanel(
                'bilinkerBilinks',
                `Bilinks (${ids.length})`,
                vscode.ViewColumn.Beside,
                {}
            );
            panel.webview.html = bilinksHtml(ids, uri);
        })
    );
}

// El visor vive en lattice: bilinker recorre cadenas, lattice compone el grafo
// de todos los proveedores y lo renderiza. La extensión sigue usando
// bilinker-lsp para hover y code lens, que sí son suyos.
function openGraph(filePath: string, recursive: boolean) {
    const lattice = findBinary('lattice');
    if (!lattice) {
        vscode.window.showErrorMessage(
            'lattice not found in PATH. Run: cargo install --path crates/lattice-cli'
        );
        return;
    }

    const isDir    = fs.statSync(filePath).isDirectory();
    const cwd      = isDir ? filePath : (vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? path.dirname(filePath));
    const selector = isDir ? '.' : path.relative(cwd, filePath);
    const args = [lattice, 'graph', selector, '--format', 'html'];
    if (recursive) args.push('--recursive');

    vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: 'Generando el grafo…' },
        () => new Promise<string>((resolve, reject) => {
            const child = spawn(args[0], args.slice(1), { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
            const chunks: Buffer[] = [];
            const errChunks: Buffer[] = [];
            child.stdout.on('data', (d: Buffer) => chunks.push(d));
            child.stderr.on('data', (d: Buffer) => errChunks.push(d));
            child.on('error', (err: Error) => reject(err));
            child.on('close', (code: number) => {
                // 3 = grafo degradado: algún proveedor no respondió. El grafo
                // es válido, así que se muestra igual con el aviso.
                if (code !== 0 && code !== 3) {
                    const stderr = Buffer.concat(errChunks).toString().trim();
                    reject(new Error(`lattice exited ${code}${stderr ? ': ' + stderr : ''}`));
                    return;
                }
                if (code === 3) {
                    vscode.window.showWarningMessage(
                        'lattice: grafo incompleto — ' +
                        Buffer.concat(errChunks).toString().trim().split('\n').pop()
                    );
                }
                resolve(Buffer.concat(chunks).toString('utf8'));
            });
        })
    ).then((html: string) => {
        const panel = vscode.window.createWebviewPanel(
            'bilinkerGraph',
            recursive ? 'Lattice: grafo del sistema' : `Lattice: ${path.basename(cwd)}`,
            vscode.ViewColumn.Beside,
            { enableScripts: true }
        );
        panel.webview.html = html;
    }, (err: Error) => {
        vscode.window.showErrorMessage(`lattice graph falló: ${err.message}`);
    });
}

function findBinary(name: string): string | undefined {
    try {
        const result = execSync(`which ${name}`, { encoding: 'utf8' }).trim();
        return result || undefined;
    } catch {
        // Try ~/.cargo/bin as fallback
        const cargo = path.join(os.homedir(), '.cargo', 'bin', name);
        return fs.existsSync(cargo) ? cargo : undefined;
    }
}

function bilinksHtml(ids: string[], uri: string): string {
    const items = ids.map(id =>
        `<li><code>${id}</code></li>`
    ).join('');
    return `<!DOCTYPE html><html><body>
    <h3>Bilinks in <code>${path.basename(uri)}</code></h3>
    <ul>${items}</ul>
    </body></html>`;
}

export function deactivate(): Thenable<void> | undefined {
    return client?.stop();
}
