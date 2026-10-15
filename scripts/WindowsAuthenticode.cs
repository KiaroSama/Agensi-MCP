using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Cryptography.Pkcs;
using System.Security.Cryptography.X509Certificates;

public static class AgensiAuthenticode
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct FileInfo
    {
        public uint Size;
        [MarshalAs(UnmanagedType.LPWStr)] public string Path;
        public IntPtr File;
        public IntPtr Subject;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct TrustData
    {
        public uint Size;
        public IntPtr Policy;
        public IntPtr Sip;
        public uint Ui;
        public uint Revocation;
        public uint Choice;
        public IntPtr File;
        public uint Action;
        public IntPtr State;
        public IntPtr Url;
        public uint Flags;
        public uint Context;
        public IntPtr Settings;
    }
    [DllImport("wintrust.dll", ExactSpelling = true, CharSet = CharSet.Unicode)]
    private static extern int WinVerifyTrust(IntPtr window, ref Guid action, ref TrustData data);

    private static void VerifyNativeHash(string path)
    {
        var info = new FileInfo { Size = (uint)Marshal.SizeOf<FileInfo>(), Path = path };
        IntPtr pointer = Marshal.AllocHGlobal(Marshal.SizeOf<FileInfo>());
        var data = new TrustData {
            Size = (uint)Marshal.SizeOf<TrustData>(), Ui = 2, Choice = 1,
            File = pointer, Action = 1,
            // Native Authenticode file binding only; certificate/signature trust is checked below.
            Flags = 0x200 | 0x1000 | 0x10
        };
        var action = new Guid("00AAC56B-CD44-11D0-8CC2-00C04FC295EE");
        try
        {
            Marshal.StructureToPtr(info, pointer, false);
            if (WinVerifyTrust(new IntPtr(-1), ref action, ref data) != 0)
                throw new CryptographicException("Native Authenticode content verification failed.");
        }
        finally
        {
            data.Action = 2;
            WinVerifyTrust(new IntPtr(-1), ref action, ref data);
            Marshal.DestroyStructure<FileInfo>(pointer);
            Marshal.FreeHGlobal(pointer);
        }
    }
    private static byte[] ReadCms(string path)
    {
        using var file = File.OpenRead(path);
        using var reader = new BinaryReader(file);
        if (file.Length < 256 || reader.ReadUInt16() != 0x5a4d)
            throw new CryptographicException("Invalid PE header.");
        file.Position = 0x3c;
        uint pe = reader.ReadUInt32();
        if (pe > file.Length - 160) throw new CryptographicException("Invalid PE offset.");
        file.Position = pe;
        if (reader.ReadUInt32() != 0x4550) throw new CryptographicException("Invalid PE signature.");
        file.Position = pe + 20;
        ushort optionalSize = reader.ReadUInt16();
        long optional = pe + 24;
        file.Position = optional;
        ushort magic = reader.ReadUInt16();
        int directory = magic == 0x20b ? 112 : magic == 0x10b ? 96 : 0;
        if (directory == 0 || optionalSize < directory + 40 || optional + optionalSize > file.Length)
            throw new CryptographicException("Invalid PE optional header.");
        file.Position = optional + directory + 32;
        uint offset = reader.ReadUInt32(), size = reader.ReadUInt32();
        if (size < 8 || size > 1024 * 1024 || offset > file.Length - size)
            throw new CryptographicException("Missing or oversized Authenticode certificate table.");
        file.Position = offset;
        uint length = reader.ReadUInt32();
        ushort revision = reader.ReadUInt16(), type = reader.ReadUInt16();
        if (length < 8 || length > size || revision != 0x200 || type != 2 || ((length + 7) & ~7U) != size)
            throw new CryptographicException("Unsupported Authenticode certificate table.");
        var cms = reader.ReadBytes(checked((int)length - 8));
        if (cms.Length != length - 8) throw new CryptographicException("Truncated Authenticode signature.");
        return cms;
    }
    public static void Verify(string path, string signerPem, string rootPem)
    {
        using var expected = X509Certificate2.CreateFromPem(File.ReadAllText(signerPem, System.Text.Encoding.UTF8));
        using var root = X509Certificate2.CreateFromPem(File.ReadAllText(rootPem, System.Text.Encoding.UTF8));
        VerifyNativeHash(path);
        var cms = new SignedCms();
        cms.Decode(ReadCms(path));
        cms.CheckSignature(true);
        if (cms.SignerInfos.Count != 1 || cms.SignerInfos[0].Certificate == null)
            throw new CryptographicException("Expected exactly one Authenticode signer.");
        var signer = cms.SignerInfos[0].Certificate;
        if (!CryptographicOperations.FixedTimeEquals(signer.GetCertHash(HashAlgorithmName.SHA256), expected.GetCertHash(HashAlgorithmName.SHA256)))
            throw new CryptographicException("Unexpected Authenticode signer.");
        using var chain = new X509Chain();
        chain.ChainPolicy.TrustMode = X509ChainTrustMode.CustomRootTrust;
        chain.ChainPolicy.CustomTrustStore.Add(root);
        chain.ChainPolicy.ExtraStore.AddRange(cms.Certificates);
        chain.ChainPolicy.ApplicationPolicy.Add(new Oid("1.3.6.1.5.5.7.3.3"));
        chain.ChainPolicy.DisableCertificateDownloads = true;
        // This private CA publishes no revocation service; no public revocation assurance is claimed.
        chain.ChainPolicy.RevocationMode = X509RevocationMode.NoCheck;
        chain.ChainPolicy.VerificationFlags = X509VerificationFlags.NoFlag;
        if (!chain.Build(signer)) throw new CryptographicException("Private code-signing certificate chain is invalid.");
    }
}
