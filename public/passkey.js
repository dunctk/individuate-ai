// Helpers for WebAuthn (Passkeys)

function base64UrlDecode(str) {
    // Add padding if needed
    let padding = '='.repeat((4 - str.length % 4) % 4);
    let base64 = (str + padding).replace(/\-/g, '+').replace(/_/g, '/');
    const rawData = atob(base64);
    const outputArray = new Uint8Array(rawData.length);
    for (let i = 0; i < rawData.length; ++i) {
        outputArray[i] = rawData.charCodeAt(i);
    }
    return outputArray;
}

function base64UrlEncode(buffer) {
    const bytes = new Uint8Array(buffer);
    let binary = '';
    for (let i = 0; i < bytes.byteLength; i++) {
        binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary)
        .replace(/\+/g, '-')
        .replace(/\//g, '_')
        .replace(/=+$/, '');
}

window.registerPasskey = async function(optionsJson) {
    try {
        console.log("Register Passkey Options:", optionsJson);
        const options = optionsJson; // Assuming optionsJson is the object from Rust

        // Convert challenge from Base64URL to Buffer
        options.publicKey.challenge = base64UrlDecode(options.publicKey.challenge);
        options.publicKey.user.id = base64UrlDecode(options.publicKey.user.id);
        
        if (options.publicKey.excludeCredentials) {
            for (let cred of options.publicKey.excludeCredentials) {
                cred.id = base64UrlDecode(cred.id);
            }
        }

        const cred = await navigator.credentials.create(options);
        
        // Convert response buffers to Base64URL for server
        const response = {
            id: cred.id,
            rawId: base64UrlEncode(cred.rawId),
            type: cred.type,
            response: {
                attestationObject: base64UrlEncode(cred.response.attestationObject),
                clientDataJSON: base64UrlEncode(cred.response.clientDataJSON),
            }
        };
        
        if (cred.authenticatorAttachment) {
            response.authenticatorAttachment = cred.authenticatorAttachment;
        }
        
        console.log("Register Response:", response);
        return response;
    } catch (e) {
        console.error("Passkey Register Error:", e);
        throw e;
    }
};

window.loginPasskey = async function(optionsJson) {
    try {
        console.log("Login Passkey Options:", optionsJson);
        const options = optionsJson;

        options.publicKey.challenge = base64UrlDecode(options.publicKey.challenge);
        
        if (options.publicKey.allowCredentials) {
            for (let cred of options.publicKey.allowCredentials) {
                cred.id = base64UrlDecode(cred.id);
            }
        }

        const cred = await navigator.credentials.get(options);

        const response = {
            id: cred.id,
            rawId: base64UrlEncode(cred.rawId),
            type: cred.type,
            response: {
                authenticatorData: base64UrlEncode(cred.response.authenticatorData),
                clientDataJSON: base64UrlEncode(cred.response.clientDataJSON),
                signature: base64UrlEncode(cred.response.signature),
                userHandle: cred.response.userHandle ? base64UrlEncode(cred.response.userHandle) : null,
            }
        };
        console.log("Login Response:", response);
        return response;
    } catch (e) {
        console.error("Passkey Login Error:", e);
        throw e;
    }
};
