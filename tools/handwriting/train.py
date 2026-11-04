"""Train Leaf Notes' offline handwriting character recognizer.

Data: EMNIST (by_merge: digits + letters) and HASYv2 (math symbols), plus synthetic
strokes for simple marks (= ( ) / - + ,). Every sample is normalised the same way the
app normalises ink: thin to a 1-px skeleton, fit into a 20x20 box (aspect kept), centre
in 28x28, thicken with a 3x3 dilation, light blur. So thickness and pen style don't matter.

The app ships two models that vote (their probabilities are averaged): assets/hw-model.bin from
this script (5 conv layers, batch norm folded in, "LNHW2") and assets/hw-model-a.bin from an earlier
4-conv run ("LNHW1"). On real handwriting turned into pen strokes the pair beat either model alone.

Usage:  CAP=12000 EPOCHS=24 python train.py <data_dir> <out_model.bin> <out_parity.bin>
Writes the weights in the little binary format read by src/hw.rs.
"""
import csv, gzip, struct, sys, os, math, random
import numpy as np
from PIL import Image
from skimage.morphology import skeletonize
from scipy.ndimage import binary_dilation, gaussian_filter, zoom
from concurrent.futures import ProcessPoolExecutor
import torch, torch.nn as nn, torch.nn.functional as F

DATA, OUT, PARITY = sys.argv[1], sys.argv[2], sys.argv[3]
CAP = int(os.environ.get('CAP', 12000))
rng = np.random.default_rng(7)

HASY = {  # latex -> label used by the app
    r'\int': '∫', r'\sum': '∑', r'\Sigma': '∑', r'\prod': '∏', r'\pi': 'π', r'\sqrt{}': '√', r'\infty': '∞',
    r'\alpha': 'α', r'\beta': 'β', r'\gamma': 'γ', r'\delta': 'δ', r'\theta': 'θ', r'\lambda': 'λ', r'\mu': 'μ',
    r'\sigma': 'σ', r'\phi': 'φ', r'\varphi': 'φ', r'\omega': 'ω', r'\Delta': 'Δ', r'\Omega': 'Ω', r'\rho': 'ρ',
    r'\tau': 'τ', r'\epsilon': 'ε', r'\varepsilon': 'ε', r'\times': '×', r'\pm': '±', r'\leq': '≤', r'\geq': '≥',
    r'\neq': '≠', r'\rightarrow': '→', r'\Rightarrow': '⇒', r'\partial': '∂', r'\nabla': '∇', r'\approx': '≈',
    r'\in': '∈', r'\div': '÷', '[': '[', ']': ']', r'\{': '{', r'\}': '}', '<': '<', '>': '>', r'\sim': '∼',
}
SYNTH = ['=', '(', ')', '/', '-', '+']


def normalise(binary):
    """binary HxW bool image -> 28x28 float in [0,1] (same steps as hw.rs)."""
    sk = skeletonize(binary)
    ys, xs = np.nonzero(sk)
    if len(xs) == 0:
        return np.zeros((28, 28), np.float32)
    x0, x1, y0, y1 = xs.min(), xs.max(), ys.min(), ys.max()
    w, h = x1 - x0 + 1, y1 - y0 + 1
    s = 20.0 / max(w, h)
    out = np.zeros((28, 28), bool)
    cx, cy = (x0 + x1) / 2.0, (y0 + y1) / 2.0
    # map every skeleton pixel and connect neighbours so scaling up leaves no gaps
    pts = {(int(y), int(x)) for y, x in zip(ys, xs)}
    def put(px, py):
        u, v = int(round((px - cx) * s + 13.5)), int(round((py - cy) * s + 13.5))
        if 0 <= u < 28 and 0 <= v < 28:
            out[v, u] = True
    for (y, x) in pts:
        put(x, y)
        for dy, dx in ((0, 1), (1, 0), (1, 1), (1, -1)):
            if (y + dy, x + dx) in pts:
                n = max(2, int(math.ceil(s)) * 2)
                for k in range(1, n):
                    t = k / n
                    put(x + dx * t, y + dy * t)
    out = binary_dilation(out, structure=np.ones((3, 3), bool))
    return gaussian_filter(out.astype(np.float32), 0.6)


def load_idx(path):
    with gzip.open(path) as f:
        data = f.read()
    magic, n = struct.unpack('>II', data[:8])
    if magic == 2051:
        r, c = struct.unpack('>II', data[8:16])
        return np.frombuffer(data, np.uint8, offset=16).reshape(n, r, c)
    return np.frombuffer(data, np.uint8, offset=8)


def synth(label):
    """Draw a simple mark as random polylines (like a person would)."""
    img = Image.new('L', (64, 64), 0)
    from PIL import ImageDraw
    d = ImageDraw.Draw(img)
    j = lambda a: a + rng.normal(0, 2.5)
    def line(pts):
        d.line([(j(x), j(y)) for x, y in pts], fill=255, width=3)
    def arc(cx, sign):
        pts = [(cx + sign * 9 * math.sin(t) + rng.normal(0, 0.8), 32 - 22 * math.cos(t)) for t in np.linspace(0.05, math.pi - 0.05, 14)]
        d.line(pts, fill=255, width=3)
    a = rng.uniform(-0.12, 0.12)
    if label == '=':
        L = rng.uniform(18, 40); g = rng.uniform(7, 14)
        line([(32 - L / 2, 32 - g / 2 + a * L), (32 + L / 2, 32 - g / 2 - a * L)]); line([(32 - L / 2, 32 + g / 2 + a * L), (32 + L / 2, 32 + g / 2 - a * L)])
    elif label == '-':
        L = rng.uniform(18, 44); line([(32 - L / 2, 32 + a * L), (32 + L / 2, 32 - a * L)])
    elif label == '+':
        L = rng.uniform(20, 36); line([(32 - L / 2, 32), (32 + L / 2, 32)]); line([(32, 32 - L / 2), (32, 32 + L / 2)])
    elif label == '(':
        arc(36, -1)
    elif label == ')':
        arc(28, 1)
    elif label == '/':
        line([(40 + rng.normal(0, 3), 10), (24 + rng.normal(0, 3), 54)])
    return normalise(np.array(img) > 127)


def emnist_item(img):
    return normalise(img.T > 100)  # EMNIST images are stored transposed


def hasy_item(path):
    return normalise(np.array(Image.open(os.path.join(DATA, path)).convert('L')) < 128)


def main():
    mapping = [chr(int(l.split()[1])) for l in open(os.path.join(DATA, 'gzip/emnist-bymerge-mapping.txt'))]
    labels = list(mapping) + sorted(set(HASY.values()), key=lambda c: list(HASY.values()).index(c)) + SYNTH
    index = {c: i for i, c in enumerate(labels)}
    print('classes', len(labels), ''.join(labels))

    cache = os.path.join(DATA, f'prepared-{CAP}.npz')
    if os.path.exists(cache):
        z = np.load(cache)
        X, Y = list(z['X']), list(z['Y'])
        print('loaded prepared data', len(X))
    else:
        X, Y = prepare(mapping, index)
        np.savez(cache, X=np.stack(X), Y=np.array(Y))
    train(X, Y, labels)


def prepare(mapping, index):
    X, Y = [], []
    imgs = load_idx(os.path.join(DATA, 'gzip/emnist-bymerge-train-images-idx3-ubyte.gz'))
    labs = load_idx(os.path.join(DATA, 'gzip/emnist-bymerge-train-labels-idx1-ubyte.gz'))
    cap = CAP  # cap per class so digits don't drown out letters
    take = []
    for c in range(len(mapping)):
        idx = np.nonzero(labs == c)[0]
        take.extend(rng.choice(idx, min(cap, len(idx)), replace=False))
    with ProcessPoolExecutor() as ex:
        X += list(ex.map(emnist_item, (imgs[i] for i in take), chunksize=512))
    Y += [index[mapping[labs[i]]] for i in take]
    print('emnist', len(take))

    rows = [r for r in csv.DictReader(open(os.path.join(DATA, 'hasy-data-labels.csv'))) if r['latex'] in HASY]
    with ProcessPoolExecutor() as ex:
        X += list(ex.map(hasy_item, (r['path'] for r in rows), chunksize=256))
    Y += [index[HASY[r['latex']]] for r in rows]
    print('hasy', len(rows))

    n_syn = 1500
    with ProcessPoolExecutor() as ex:
        for lab in SYNTH:
            X += list(ex.map(synth, [lab] * n_syn, chunksize=64))
            Y += [index[lab]] * n_syn
    print('synthetic', n_syn * len(SYNTH))
    return X, Y


def train(X, Y, labels):
    # Train on the GPU when there is one; preprocessing above is CPU work either way.
    dev = torch.device('cuda' if torch.cuda.is_available() else 'cpu')
    print('training on', torch.cuda.get_device_name(0) if dev.type == 'cuda' else 'CPU')
    X = torch.tensor(np.stack(X)).unsqueeze(1)
    Y = torch.tensor(Y)
    perm = torch.randperm(len(X), generator=torch.Generator().manual_seed(1))
    X, Y = X[perm], Y[perm]
    n_val = 6000
    X, Y = X.to(dev), Y.to(dev)
    Xv, Yv, Xt, Yt = X[:n_val], Y[:n_val], X[n_val:], Y[n_val:]

    class Net(nn.Module):
        """Five 3x3 conv layers (each conv → batch norm → ReLU), three 2x2 max-pools, two dense layers."""
        def __init__(s, c):
            super().__init__()
            chans = [(1, 32), (32, 32), (32, 64), (64, 64), (64, 128)]
            s.convs = nn.ModuleList([nn.Conv2d(a, b, 3, padding=1) for a, b in chans])
            s.bns = nn.ModuleList([nn.BatchNorm2d(b) for _, b in chans])
            s.f1, s.f2 = nn.Linear(128 * 3 * 3, 256), nn.Linear(256, c)
            s.drop = nn.Dropout(0.35)
        def forward(s, x):
            for i, (cv, bn) in enumerate(zip(s.convs, s.bns)):
                x = F.relu(bn(cv(x)))
                if i in (1, 3, 4):
                    x = F.max_pool2d(x, 2)
            return s.f2(s.drop(F.relu(s.f1(x.flatten(1)))))

    torch.manual_seed(0)
    torch.set_num_threads(os.cpu_count())
    net = Net(len(labels)).to(dev)
    # weight classes inversely to frequency (softened) so rare symbols still count
    counts = torch.bincount(Yt, minlength=len(labels)).float()
    weight = (counts.mean() / counts.clamp(min=1)) ** 0.5
    opt = torch.optim.AdamW(net.parameters(), 2e-3, weight_decay=2e-4)
    epochs = int(os.environ.get('EPOCHS', 24))
    bs = 512
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, 4e-3, total_steps=epochs * (len(Xt) // bs + 1))
    def augment(xb):
        n = len(xb)
        # rotation, shear, scale, shift …
        th = (torch.rand(n, device=dev) - 0.5) * 0.35
        sh = (torch.rand(n, device=dev) - 0.5) * 0.5
        sc = 1 + (torch.rand(n, device=dev) - 0.5) * 0.25
        A = torch.zeros(n, 2, 3, device=dev)
        A[:, 0, 0] = torch.cos(th) * sc; A[:, 0, 1] = -torch.sin(th) * sc + sh; A[:, 1, 0] = torch.sin(th) * sc; A[:, 1, 1] = torch.cos(th) * sc
        A[:, :, 2] = (torch.rand(n, 2, device=dev) - 0.5) * 0.15
        grid = F.affine_grid(A, xb.shape, align_corners=False)
        # … plus a gentle random warp, like the wobble of a real hand
        warp = F.interpolate((torch.rand(n, 2, 4, 4, device=dev) - 0.5) * 0.16, size=(28, 28), mode='bicubic', align_corners=False)
        xb = F.grid_sample(xb, grid + warp.permute(0, 2, 3, 1), align_corners=False)
        # small gaps, like a pen skipping
        keep = torch.ones_like(xb)
        hit = torch.rand(n, device=dev) < 0.3
        cx, cy = torch.randint(4, 24, (n,), device=dev), torch.randint(4, 24, (n,), device=dev)
        yy, xx = torch.meshgrid(torch.arange(28, device=dev), torch.arange(28, device=dev), indexing='ij')
        gap = ((xx[None] - cx[:, None, None]).abs() <= 1) & ((yy[None] - cy[:, None, None]).abs() <= 1) & hit[:, None, None]
        keep[gap[:, None]] = 0.0
        return xb * keep
    for ep in range(epochs):
        net.train()
        p = torch.randperm(len(Xt), device=dev)
        tot = 0.0
        for b in range(0, len(Xt), bs):
            idx = p[b:b + bs]
            xb, yb = augment(Xt[idx]), Yt[idx]
            loss = F.cross_entropy(net(xb), yb, weight=weight, label_smoothing=0.05)
            opt.zero_grad(); loss.backward(); opt.step(); sched.step()
            tot += loss.item() * len(idx)
        net.eval()
        with torch.no_grad():
            pred = torch.cat([net(Xv[i:i + 1024]).argmax(1) for i in range(0, len(Xv), 1024)])
        print(f'epoch {ep + 1}: loss {tot / len(Xt):.3f}  val acc {(pred == Yv).float().mean():.4f}', flush=True)

    # per-class accuracy on the held-out set (informational)
    with torch.no_grad():
        pred = torch.cat([net(Xv[i:i + 1024]).argmax(1) for i in range(0, len(Xv), 1024)])
    worst = sorted(((((pred == Yv) & (Yv == c)).sum().item() / max(1, (Yv == c).sum().item()), labels[c]) for c in range(len(labels))))[:12]
    print('weakest classes', worst)

    # fold each batch norm into its convolution so the app only needs conv + ReLU
    net = net.cpu().eval()
    tensors = []
    for cv, bn in zip(net.convs, net.bns):
        k = bn.weight / torch.sqrt(bn.running_var + bn.eps)
        tensors += [cv.weight * k[:, None, None, None], (cv.bias - bn.running_mean) * k + bn.bias]
    tensors += [net.f1.weight, net.f1.bias, net.f2.weight, net.f2.bias]
    with open(OUT, 'wb') as f:
        lab = '\n'.join(labels).encode()
        f.write(b'LNHW2'); f.write(struct.pack('<II', len(labels), len(lab))); f.write(lab)
        for t in tensors:
            f.write(t.detach().numpy().astype('<f4').tobytes())
    # a few validation samples + PyTorch's answers so the Rust port can be checked for parity
    with torch.no_grad():
        logits = net(Xv[:64].cpu())
    with open(PARITY, 'wb') as f:
        f.write(struct.pack('<I', 64))
        f.write(Xv[:64].cpu().numpy().astype('<f4').tobytes())
        f.write(logits.argmax(1).numpy().astype('<u4').tobytes())
    print('wrote', OUT, os.path.getsize(OUT), 'bytes')


if __name__ == '__main__':
    main()
