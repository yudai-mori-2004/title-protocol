# RootLens Terms of Service

**Version 1.0.0**

---

## At a glance (plain-language summary, non-binding)

This is a plain-language summary of the most important points. The numbered Terms below control if there is any difference.

- **You keep the copyright.** Uploading and minting doesn't transfer ownership.
- **We run the marketplace; you are the seller.** Buyers get a copy of your file plus a "License NFT" that records what they're allowed to do with it. Buyers pay in USDC, directly to your wallet, minus any platform fee we have disclosed.
- **You set the price.** We may suggest a price computed from market data; you can override it before each sale.
- **You promise the Content is yours, lawful, and free of third-party rights.** If a third party sues us because that wasn't true, you cover our reasonable costs (Section 10).
- **AI training is opt-in only.** Your Content carries a machine-readable "do not train" signal. The only way to permit AI training is to sell a `training-only-v1` licence. We do not warrant what the buyer's AI then produces — that depends on the buyer's design (Section 6.3A).
- **Minting is permanent.** Once your Root NFT is on Solana, neither you nor we can delete it.
- **Stopping or cancelling.** Stop using the app to stop new submissions. To stop future License NFT issuance against a Root NFT you already minted, transfer that Root NFT to a burn address or to a wallet you don't control (Section 6.6). License NFTs already issued continue under their own terms.
- **We are infrastructure, not insurance.** We do not compensate losses beyond what the law forces us to (Section 12). Responsibility is layered. You represent ownership. We operate the KYC process and the technical pipeline (Section 4.3). The buyer is responsible for compliant use of the licence.
- **Dispute resolution.** SIAC arbitration in Singapore, individual cases, no class actions (Section 16). **If you are a consumer in the EEA, UK, Switzerland, or Japan, you keep the right to sue in your local courts and to use local consumer protections** (Section 2A).
- **Reporting illegal Content, or asking for support.** Email the address in Section 18.
- **Versioning.** Each Root NFT is locked to the version of these Terms in force when you minted it. Future versions don't change past mints.
- **If we shut down**, your Root NFTs and License NFTs continue to exist on Solana. We will try to give 90 days' notice and to preserve the licence terms via decentralised storage (Section 15.3).
- **What you see in the app is what binds.** If the app accidentally shows a different version, the version you actually saw wins — but only if that doesn't reduce your rights (Section 17.9(d)).

---

## 1. Definitions

In these Terms, capitalised words have the meanings given below.

1.1 **"RootLens"**, **"we"**, **"us"**, or **"our"** means the operator identified in Section 18, currently a sole proprietorship operating under the trade name "moodai".

1.2 **"You"** or **"Creator"** means the natural person who accepts these Terms and submits Content.

1.3 **"Content"** means an image, video, or other digital file captured using the RootLens Application and submitted by you for processing.

1.4 **"RootLens Application"** means the mobile or web application we publish through which Creators capture and submit Content.

1.5 **"TEE"** means the Trusted Execution Environment we operate, running the Title Protocol software, that processes Content and produces a cryptographic attestation.

1.6 **"Root NFT"** means a non-fungible token on the Solana blockchain minted by the TEE for a specific item of Content, representing that the Content has passed through the framework defined by these Terms.

1.7 **"License NFT"** means a separate Solana NFT issued under one of the standardised licence templates listed in Section 6.3 that grants specified rights in a specific item of Content to the holder of that License NFT.

1.8 **"Content Authenticity Signal"** or **"Signal"** means the machine-readable mark (technically, a CAWG training-mining assertion) embedded in the C2PA manifest attached to the Content, reserving rights against text-and-data mining (**"TDM"**) and AI training under Article 4(3) of EU Directive 2019/790 (the **"CDSM Directive"**) and analogous laws.

1.9 **"Effective Version"** means the version of these Terms recorded inside the TEE's signed attestation at the moment your Root NFT was minted. This is the version that applies to that specific Root NFT for its lifetime.

1.10 **"Authoritative Hash"** means a SHA-256 fingerprint that uniquely identifies one exact version of these Terms, computed as described in Section 17.9.

1.11 **"Consumer"** means a natural person acting outside the scope of their trade, business, craft, or profession.

## 2. Acceptance

2.1 The RootLens Application presents two separate consent screens before processing your Content:

   (a) **Terms acceptance.** By tapping "I Agree to the Terms" on the first screen, you accept these Terms as a legally binding agreement between you and us. This step concerns only the contract.

   (b) **Personal data processing.** The second screen describes what personal data we process to evidence your acceptance (your wallet public key, IP address, user agent, and timestamp), the legal basis for each category of processing, and your rights. You can read these Terms and submit Content without making any optional choices on this screen; data processing necessary to perform this contract and to evidence your acceptance proceeds on the legal bases stated in Section 13, not on consent.

2.2 If you do not accept these Terms, the RootLens Application will not transmit Content to the TEE.

2.3 Each act of submission is also an act of confirmation that the Effective Version of these Terms applies to that submission. The Effective Version for a submission is the version constant embedded in the TEE binary that processes the submission, as recorded in the resulting attestation.

2.4 **EU/UK Consumer withdrawal acknowledgement.** If you are a Consumer habitually resident in the EEA, UK, or Switzerland, you have a statutory right to withdraw from a distance contract within 14 days. Minting a Root NFT is an irreversible on-chain operation. By tapping "Mint Now", you expressly request that we begin performance immediately and acknowledge that, once minting begins, you lose your 14-day right of withdrawal under Article 16(m) of Directive 2011/83/EU and regulations 36 and 37 of the UK Consumer Contracts (Information, Cancellation and Additional Charges) Regulations 2013.

## 2A. Consumer Carve-Out (mandatory law and forum)

2A.1 **Scope.** This Section 2A applies to any Creator who is a Consumer habitually resident in the European Economic Area, the United Kingdom, Switzerland, or Japan. Where this Section conflicts with any other provision of these Terms, this Section prevails.

2A.2 **Mandatory law preserved.** Our choice of Singapore law in Section 16.1 does not deprive you of the protection of mandatory provisions of the law of your country of habitual residence (Rome I Regulation Article 6(2); equivalent UK and Japanese rules).

2A.3 **Forum preserved.** Sections 16.3, 16.4, and 16.5 (SIAC arbitration, seat, individual proceedings) do not apply to you. You may bring proceedings in the courts of your country of residence. We may bring proceedings against you only in those courts (Brussels I Recast Article 18; Japanese Civil Procedure Act Article 3-7(5); Japanese Arbitration Act Supplementary Provision Article 3).

2A.4 **Collective redress preserved.** Section 16.5 shall not be read as a waiver of any right you have under EU Directive 2020/1828 (Representative Actions), Article 80 GDPR, the Japanese Consumer Class Action Special Procedures Act, the right of qualified consumer organisations to seek injunctive relief under Article 12 of the Japanese Consumer Contract Act, or equivalent local law.

2A.5 **Mandatory-law floor.** Section 12 does not limit our liability to you for (a) death or personal injury caused by our negligence, (b) fraud or fraudulent misrepresentation, (c) gross negligence or wilful misconduct, (d) breach of statutory consumer rights under EU Directive 2019/770, EU Directive 2019/771, UK Consumer Rights Act 2015 Parts 1 and 2, the Japanese Consumer Contract Act Articles 8 and 8-2, or equivalent local law, or (e) any liability that the applicable mandatory law does not permit to be limited. We won't try to do less than the law in your country requires of us, and we don't promise to do more.

2A.6 **Statutory remedies preserved.** Section 11 does not exclude or limit any statutory remedy you have under EU Directive 2019/770, the UK Consumer Rights Act 2015 Part 1, the Japanese Consumer Contract Act Article 8-2, or equivalent local law.

2A.7 **Indemnity cap.** Your obligation under Section 10 is capped at the aggregate net proceeds you have received from License NFTs issued in respect of your Content, except for breach of Sections 5.1, 5.2, or 5.3 (authorship, originality, and consent of depicted persons), where the indemnity remains uncapped.

2A.8 **Language (Japanese Consumers).** If you are a Consumer habitually resident in Japan, the Japanese version of these Terms controls over the English version in case of conflict.

## 3. Eligibility

3.1 You represent that you are at least 18 years old, or the age of majority in your jurisdiction, whichever is higher.

3.2 You represent that:

   (a) you are not located in, ordinarily resident in, or a national of any country subject to comprehensive sanctions administered by the U.S. Office of Foreign Assets Control, the European Union, the United Kingdom, the Monetary Authority of Singapore, or the United Nations Security Council; and

   (b) you are not named on any sanctions list maintained by any of those authorities, nor are you targeted under the Japanese Foreign Exchange and Foreign Trade Act, the Singapore Terrorism (Suppression of Financing) Act 2002, or any equivalent law.

3.3 You represent that your use of the RootLens Application does not violate the laws of your jurisdiction of residence.

3.4 If any representation in this Section 3 becomes untrue after you accept these Terms, you must immediately stop using the RootLens Application.

3.5 **EEA market.** The RootLens Application is not actively offered to users habitually resident in the European Economic Area. If you nevertheless use the Application from the EEA, the protections in Section 2A apply, but we do not target the EEA market within the meaning of Article 3(2)(a) of the GDPR.

## 4. Identification

4.1 We identify you by the Solana wallet public key that signs your mint transaction. That key is your identity for the purpose of these Terms.

4.2 You are solely responsible for the security of your wallet's private key. Loss of the private key is permanent loss of control over any Root NFT minted from the corresponding wallet. We cannot recover lost keys.

4.3 **Identity verification (KYC).** Before you may submit Content for the first time, you must complete identity verification ("KYC") through the means we provide. We may suspend or refuse submissions from wallets whose holder has not completed KYC, or whose KYC has been revoked by us or by our KYC provider. Identity information collected through KYC is stored encrypted, separated from your operational account, and used only for (a) legitimate dispute resolution between Creator, buyer, and us; (b) anti-money-laundering compliance; (c) fraud prevention and platform abuse mitigation; (d) compliance with tax or other legal obligations; and (e) attribution of liability where you breach Section 5. The Privacy Policy gives the detail on retention, access, and your rights.

## 5. Your Promises about the Content

By submitting Content you make the following promises. We rely on them, and so do purchasers of any License NFT issued under your Root NFT.

5.1 **You are the author.** You are the natural-person author of the Content, or you hold a complete and unencumbered chain of title from the author with full authority to grant the licences contemplated by Section 6. Where you are not the original author, your chain of title includes the right to assert reservations of rights against text and data mining and AI training under Article 4(3) of the CDSM Directive, the proviso to Article 30-4 of the Japanese Copyright Act, and analogous laws.

5.2 **The Content is original.** The Content was captured by you using the RootLens Application. The Content does not incorporate, reproduce, or derive from any third party's copyrighted work, trademark, trade dress, right of publicity, or other proprietary right.

5.3 **No other people without consent.** The Content does not show or record any identifiable person other than yourself, unless you hold a written release from that person sufficient to authorise the licences in Section 6. You will produce that release at our request.

5.4 **The Content is lawful.** The Content is not defamatory, harassing, threatening, obscene, or otherwise unlawful under the law of any jurisdiction where the Content is made available through the RootLens framework.

5.5 **No conflicting grants.** You have not granted any third party rights in the Content that would conflict with the licences in Section 6. The Content is not subject to any exclusive licence, assignment, lien, or encumbrance.

5.6 **You can sign this contract.** You have the legal capacity to enter into binding contracts under the law of your residence.

If you break these promises, Section 10 (Indemnification) applies.

## 6. Marketplace and Issuance Facilitation

6.1 **Authorisation as marketplace operator and issuance facilitator.** By submitting Content and accepting these Terms, you authorise us to act as a **non-exclusive marketplace operator and on-chain issuance facilitator** for License NFTs derived from the Root NFT minted from that Content. You remain the licensor of every licence granted. We do not grant licences on your behalf; we provide the technical means by which your unilateral licence grants are recorded on-chain.

6.1A **No assumption of grantor liability.** We do not become a party to any licence granted by a License NFT. We make no representation or warranty to any holder of a License NFT regarding the Content. You are the sole grantor and the sole party liable on the licence.

6.1B **Scope of facilitator duties.** The parties intend that our duties as issuance facilitator are limited to those expressly set out in these Terms. We do not manage your copyrights, do not negotiate licences on your behalf, and do not determine licence terms or prices. No fiduciary duties are implied beyond those mandatory under Singapore law.

6.2 **What the buyer receives, and what you keep.** A buyer of a License NFT typically receives a copy of the Content data, together with the bundle of rights that the corresponding licence template defines. In economic terms this is a sale; in legal terms it is a licence. We use a licence structure because copyright law, taken together with the realities of digital distribution (data is freely copyable, hard to track, and impossible to recall after delivery), makes outright transfer of the underlying data an unsuitable legal construct. Under these Terms:

   (a) you do not assign or transfer any copyright in the Content; copyright stays with you;
   (b) the buyer's lawful use of the Content is bounded by the licence template; uses outside that scope are unauthorised and may be the subject of a copyright claim by you. We do not enforce your copyright on your behalf, and any enforcement engagement, if separately agreed in writing, is outside the scope of these Terms; and
   (c) we cannot guarantee that buyers will stay within their licence. Enforcement against unauthorised use is a separate matter, governed by the licence template and applicable copyright law.

6.3 **Permitted licence templates.** As of the Effective Version of these Terms, the permitted licence templates are:

   (a) **commercial-v1** — commercial use of the Content;
   (b) **non-commercial-v1** — non-commercial use of the Content;
   (c) **training-only-v1** — use of the Content for AI model training and text and data mining;
   (d) **redistribution-v1** — redistribution of the Content.

   Each template is content-addressed at `https://rootlens.io/licenses/{type}/{hash}.json`. The terms of each template are incorporated by reference and form part of the licence granted by the corresponding License NFT.

6.3A **Scope of licensed use.** Each licence template authorises specific uses of the Content as a licensed input — for example, AI model training in the training-only-v1 template. Neither you (as licensor) nor we (as marketplace operator) warrant that any output a buyer produces from a downstream use, including any content generated by an AI model trained on the Content, is itself non-infringing or compliant with applicable law. Output compliance depends on the buyer's design and on the totality of inputs the buyer uses, both of which are outside your and our control.

6.4 **Unilateral grant.** Each License NFT operates as a unilateral grant from you to the holder of that License NFT on the terms recited in the corresponding template. We facilitate issuance; you are the grantor.

6.4A **Third-party rights.** For the purposes of the Singapore Contracts (Rights of Third Parties) Act 2001 and equivalent laws, the holder from time to time of a License NFT is intended to have the benefit of, and may enforce, (i) the terms of the corresponding licence template against you as Creator, and (ii) to the extent expressly stated in Section 10.1, the indemnity in Section 10.1. No other person is intended to have any benefit or right under these Terms.

6.5 **Non-exclusivity.** Our role under Section 6.1 is non-exclusive. You may grant licences in the same Content outside the RootLens framework, provided you do not breach Section 5.5.

6.6 **Revocation of authorisation.** You may revoke this authorisation for future issuances by (a) transferring the Root NFT to a Solana null address or (b) transferring it to a wallet you do not control. License NFTs already issued before revocation remain valid on their own terms.

6.7 **Price-setting and platform fee.**

   (a) **You set the price and terms of each issuance.** For every License NFT issued under your Root NFT, you, the Creator, are the person who sets the price and the non-template-default terms. The RootLens Application provides a price-setting screen on which you may set the price at any time, for any individual License NFT or as a default for all License NFTs of a given template type. We do not set the price on your behalf.

   (b) **Suggested market default.** Where you have not actively set a price, the RootLens Application may display a non-binding suggested default price derived from market reference data. If you do not modify the suggestion before authorising issuance, by authorising issuance you adopt the suggestion as the price you set for that issuance. You are free to accept, modify, or override the suggestion. The suggestion is a recommendation, not a price set by us. The methodology by which the suggested default is computed is published at `https://rootlens.io/pricing-methodology` and is non-discretionary. We will not modify the methodology in a way that biases against any individual Creator or class of Creators. The RootLens Application will at all times allow you to override the suggestion with any non-negative price, including a price materially below or above the suggestion, before authorising issuance.

   (c) **Effect of your price choices.** A price that diverges materially from market reference data is permitted but may substantially reduce the likelihood that the License NFT is purchased. That is how pricing works in any marketplace; we are not predicting how sales will go.

   (d) **Platform fee.** If we charge a platform fee for an issuance, the fee is a flat amount disclosed in the Application before you authorise issuance. The fee is not a commission calculated on the licence price. Accepting these Terms does not by itself create any fee obligation; a fee, if any, attaches to a specific issuance event.

6.8 **Payment.** License NFT purchases are settled in **USDC** (the US dollar-pegged stablecoin) on Solana. When a buyer authorises a purchase, the buyer pays the price set under Section 6.7 directly to the Solana wallet associated with your Root NFT. The on-chain Solana transaction is the authoritative record of payment. Once the transaction reaches the `finalized` commitment level on Solana, payment is irrevocable; we cannot reverse it. We are not a payment processor or escrow agent. The platform fee under Section 6.7(d), if any, is paid by you separately to us as a service fee for marketplace facilitation and does not pass through our control during settlement of the licence price. You are responsible for the tax and reporting consequences of receiving USDC.

6.9 **Regulatory cessation.** Notwithstanding Section 6.1, we may suspend the facilitation of new License NFT issuances in any jurisdiction, or globally, where we reasonably believe such facilitation is or has become unlawful or subject to material regulatory risk. Such suspension, where reasonably and proportionately taken, is not by itself a breach of our obligations under this Section 6. We may resume facilitation when the regulatory concern is resolved. License NFTs already issued before suspension are not affected.

6.10 **New licence types.** We may introduce additional licence templates in future versions of these Terms. New templates do not apply to your existing Root NFTs unless you re-consent to a later version of these Terms.

## 7. Text and Data Mining Reservation

7.1 The RootLens Application embeds the Signal in the C2PA manifest of your Content. The Signal is a machine-readable reservation of rights under, among others:

   (a) Article 4(3) of the CDSM Directive;
   (b) the proviso to Article 30-4 of the Japanese Copyright Act; and
   (c) the United Kingdom's Copyright, Designs and Patents Act 1988 (under which commercial TDM requires a licence; CDPA section 29A creates only a narrow non-commercial-research exception).

7.2 The **training-only-v1** licence template is the only mechanism within the RootLens marketplace by which you grant, and a buyer may acquire, rights to use the Content for AI training or text and data mining. Any such use outside of a valid training-only-v1 License NFT is unauthorised.

7.3 The TEE attestation records that the Signal was present in the C2PA manifest of your Content when it entered the marketplace. You may use that attestation as evidence in any action you bring to enforce the reservation in Section 7.1. We do not enforce the reservation on your behalf and do not represent you in any such action. We may publish or republish the TEE attestation as a public fact, but we are not enforcing your rights when we do so.

## 8. Moral Rights

8.1 You retain all moral rights in the Content to the fullest extent permitted by law, including under Article 6bis of the Berne Convention and Articles 18 to 20 of the Japanese Copyright Act.

8.2 **Non-assertion covenant (not a waiver).** Where the use authorised by a License NFT would, if you asserted a moral right, prevent that use, you contractually undertake not to assert that moral right against the buyer to the extent necessary to give effect to the licence. This is a non-assertion covenant, not a waiver of the right itself; you retain ownership of the right and may continue to assert it outside the scope of the licence. This structure follows established practice in the music, film, and publishing industries. Nothing in this Section overrides moral rights whose non-assertion is prohibited by mandatory law of your habitual residence.

## 9. Technical Processing

9.1 When you submit Content, the RootLens Application transmits it to the TEE for processing.

9.2 The TEE (a) verifies the C2PA signature chain attached to the Content; (b) confirms that the Signal is present; (c) produces a signed attestation binding the verification results to the RootLens framework metadata; and (d) signs a transaction minting the Root NFT.

9.3 The TEE does not retain the Content. The TEE is stateless and discards the Content after processing. We do not retain a copy of the Content beyond what is strictly necessary to process your submission, except where retention is required by law.

9.4 The mint operation does not create or transfer any copyright. It creates a digital token that references your Content and records the framework version under which licences may subsequently be issued.

## 10. Indemnification

10.1 You will indemnify and hold harmless us, our affiliates, our and their officers, employees, and contractors, and any holder of a License NFT who relied in good faith on your promises (each, an "Indemnified Party"), against any claim, loss, liability, damage, cost, or expense, including reasonable legal fees ("Loss"), that arises out of or relates to the matters in subsections (a) to (c) below, and you will reimburse each Indemnified Party for its reasonable defence costs as they are incurred:

   (a) your breach of any promise in Section 5;
   (b) any third-party claim that the Content infringes copyright, trademark, the right of publicity, privacy rights, or any other right of a third party;
   (c) your breach of any other provision of these Terms.

10.2 The Indemnified Party will give you prompt written notice of any claim subject to indemnification. Where the Indemnified Party is RootLens, RootLens will control the defence and may select counsel, at your cost; you may participate at your own additional cost. Where the Indemnified Party is a holder of a License NFT, that holder controls its own defence, and your indemnity obligation is limited to reimbursement of the holder's reasonable, documented legal fees and adverse-party costs. You and the Indemnified Party will reasonably co-operate. Where you are a Consumer to whom Section 2A applies, you may instead elect to control the defence of any claim with counsel of your choice, in which case our reimbursement obligation is limited to reasonable counsel fees up to the cap in Section 2A.7.

10.3 The Indemnified Party may not settle any claim subject to indemnification in a way that imposes a non-monetary obligation on you, or admits your liability, without your prior written consent (not to be unreasonably withheld). You may not settle any claim in a way that imposes any obligation on, or admits any liability of, an Indemnified Party, without that party's prior written consent.

10.4 Section 2A.7 caps your obligation under this Section if you are a Consumer to whom Section 2A applies.

10.5 This Section 10 survives termination of these Terms.

## 11. Disclaimers

> **THESE ARE THE LIMITS ON OUR PROMISES. WE PUT THEM IN CAPITALS SO YOU CAN'T MISS THEM. SECTION 2A LIMITS HOW THIS SECTION APPLIES TO CONSUMERS.**

11.1 **"AS IS" — NO WARRANTIES.** To the maximum extent permitted by law, the RootLens Application, the TEE, and the Root NFT and License NFT framework are provided **"AS IS"** and **"AS AVAILABLE"**. We disclaim all warranties — express, implied, or statutory — including merchantability, fitness for a particular purpose, title, and non-infringement.

11.2 **BLOCKCHAIN RISKS.** We do not warrant that the Solana blockchain, the TEE, or any third-party infrastructure will be uninterrupted, error-free, or secure against all attacks. Blockchain technology is relatively new and is subject to risks of attack, fork (when a blockchain splits into two), congestion, and regulatory change.

11.3 We do not provide legal advice. Whether minting a Root NFT or acquiring a License NFT has any particular legal effect in your jurisdiction depends on facts and on local law that are outside our control. You are responsible for obtaining independent legal advice as you consider appropriate.

11.4 **Statutory remedies preserved.** Nothing in this Section excludes or limits (a) any liability that cannot be excluded or limited under applicable mandatory law, (b) liability for death or personal injury caused by our negligence, (c) liability for fraud or fraudulent misrepresentation, or (d) any statutory consumer right preserved by Section 2A.

## 12. Limitation of Liability

> **THIS SECTION LIMITS WHAT YOU CAN RECOVER FROM US. SECTION 2A.5 PRESERVES YOUR MANDATORY-LAW PROTECTIONS IF YOU ARE A CONSUMER.**

12.1 **INFRASTRUCTURE, NOT INSURANCE.** Our role is to make responsibility traceable, not to underwrite losses. We do not guarantee compensation for losses you incur through use of the RootLens Application, the TEE, Root NFTs, or License NFTs. To the maximum extent permitted by law, our liability to you arising out of or relating to these Terms — whether in contract, tort (including negligence), statute, or otherwise — is **limited to the minimum amount the applicable mandatory law requires us to pay, if any**.

12.2 **NO INDIRECT DAMAGES.** In no event will we be liable for any indirect, incidental, special, consequential, exemplary, or punitive damages, or for any loss of profit, revenue, business, data, or goodwill, even if we have been advised of the possibility of such damages.

12.3 The limit in Section 12.1 does not apply to:

   (a) liability that cannot be limited under applicable mandatory law;
   (b) fraud, fraudulent misrepresentation, or wilful misconduct;
   (c) our gross negligence;
   (d) death or personal injury caused by our negligence (UCTA 1977 s.2(1); analogous Singapore law);
   (e) any statutory consumer right preserved by Section 2A.

## 13. Privacy

13.1 Our processing of personal data is set out in our Privacy Policy at `https://rootlens.io/privacy-policy`, which is part of these Terms.

The Privacy Policy is versioned and identified by its own SHA-256 fingerprint. That fingerprint is recorded inside the TEE attestation for your submission alongside the Authoritative Hash of these Terms. The version of the Privacy Policy whose fingerprint is recorded in the attestation is the version that applies to your submission. Later updates do not retroactively change the privacy terms that applied to Content you have already submitted.

If you exercise your right of access under Article 15 of the GDPR (or equivalent), we will, on request, give you (a) the fingerprint of the Privacy Policy version recorded in your attestation and (b) the text of that specific version as published. The fingerprint recorded in the attestation is conclusive on which version applies to that submission.

13.2 **Legal bases for processing.** We do not rely on consent under Article 6(1)(a) GDPR (or equivalent) for the processing described in this Section. The legal bases are:

   (a) **Performance of the contract (Article 6(1)(b) GDPR).** Processing your wallet public key and submission metadata to mint the Root NFT and operate the licensing framework.
   (b) **Legitimate interests (Article 6(1)(f) GDPR).** Retaining the consent log (IP address, user agent string, timestamp, and the hash of the Effective Version you accepted) for the purpose of evidencing your acceptance of these Terms. Our legitimate interest in maintaining evidence of contract formation is, on balance, not overridden by your interests because (i) the data set is the minimum needed to attribute acceptance, (ii) the data is access-controlled and not used for any secondary purpose, and (iii) you reasonably expect us to keep evidence of a contract you entered.
   (c) **Compliance with legal obligation (Article 6(1)(c) GDPR).** Where applicable law requires retention (for example, tax or anti-money-laundering law).

13.3 **Effect of withdrawal of any optional consent.** If you have given a separate, optional consent for any processing not listed above (for example, analytics), you may withdraw it at any time through the application settings. Withdrawal does not affect (a) processing that took place before withdrawal or (b) processing under Section 13.2, which does not rely on consent.

13.4 **Solana blockchain is public.** You acknowledge that the Solana blockchain is a public, append-only ledger. Your wallet public key and any on-chain transaction data will be publicly visible and cannot be deleted by us. Where you exercise the right of erasure under Article 17 GDPR or equivalent, we will delete personal data within our control; we cannot delete data published on the Solana blockchain.

13.5 **Your rights.** You have the rights of access, rectification, erasure (subject to Section 13.4), restriction, portability, and objection set out in Articles 15 to 22 GDPR, and the analogous rights under the UK GDPR, the Japanese Act on the Protection of Personal Information, and the Singapore Personal Data Protection Act. Requests may be made to the contact address in Section 18.

## 14. Changes to These Terms

14.1 We may issue a new version of these Terms only for one or more of the following reasons:

   (a) compliance with a change in applicable law or regulator guidance;
   (b) a material change in the service, technology, or supplier arrangements;
   (c) correction of an error or ambiguity;
   (d) introduction of additional licence templates under Section 6.10; or
   (e) improvements that do not (i) reduce any right we grant you under these Terms, (ii) increase any obligation you owe us, (iii) reduce the cap on our liability stated in Section 12 or 2A.5, or (iv) expand the scope of our role as marketplace operator or issuance facilitator under Section 6.

14.2 We will publish a new version and notify you through the RootLens Application at least thirty (30) days before it takes effect, except where a shorter period is required by law (in which case we will give as much notice as is reasonable).

14.3 A new version applies only to Content submissions made after the new version takes effect. A new version does not change the terms of any Root NFT already minted under an earlier version. Each Root NFT remains bound by its Effective Version.

14.4 If you do not accept a new version, you may continue to hold and transfer Root NFTs already minted under earlier versions. You will not be able to submit new Content until you accept the new version.

## 15. Termination

15.1 You may stop using the RootLens Application at any time. Stopping use does not by itself revoke our authority under Section 6 in respect of Root NFTs already minted. To revoke, follow Section 6.6.

15.2 We may suspend or end your access to the RootLens Application, with or without notice, if we reasonably believe you have breached these Terms or applicable law, or that continued service would expose us or License NFT holders to material legal risk.

15.3 **If we cease operations.** If we permanently cease operating the RootLens service, we will (a) where reasonably possible, give at least 90 days' prior notice through the Application, or, where the Application is no longer operating, by email to the address you most recently provided; (b) take reasonable steps to preserve the licence template documents referenced by your License NFTs, for example by mirroring them to decentralised storage; and (c) where a successor entity continues the service under Section 17.4, transfer operations to that successor. Root NFTs and License NFTs already issued continue to exist on the Solana blockchain regardless of our operational status.

15.4 Sections 2A, 4.3, 5, 6 (in respect of Root NFTs minted before termination), 7, 8, 10, 11, 12, 13, 16, 16A, 17, and 18 survive termination.

## 16. Governing Law and Dispute Resolution

> **SECTION 2A LIMITS HOW THIS SECTION APPLIES TO CONSUMERS HABITUALLY RESIDENT IN THE EEA, UK, SWITZERLAND, OR JAPAN.**

16.1 These Terms, and any dispute (including non-contractual disputes) arising out of or in connection with them, are governed by the laws of the Republic of Singapore, without regard to its rules on conflict of laws. The law governing the arbitration agreement in this Section is also Singapore law.

16.1A **Arbitrability carve-out.** Notwithstanding Section 16.3, any claim or matter that is non-arbitrable as a matter of mandatory law of (a) the place where enforcement is sought, or (b) the law of your habitual residence, is excluded from this arbitration agreement and may be pursued in the competent court. Without limitation, this excludes regulatory proceedings before Japanese authorities (including under the Act on the Protection of Personal Information and the Financial Instruments and Exchange Act) and any other proceeding that is non-arbitrable under the law of the place of enforcement.

16.2 **Informal resolution first.** Before commencing arbitration, the parties will attempt in good faith to resolve the dispute by negotiation for thirty (30) days following written notice. Notice to us is sent to the address in Section 18; notice to you may be sent to any contact address you have given us. This Section does not revive or extend any limitation period.

16.3 Any dispute not resolved under Section 16.2, including any question regarding the existence, validity, or termination of these Terms or of this arbitration agreement, and any non-contractual dispute or claim, will be referred to and finally resolved by arbitration administered by the Singapore International Arbitration Centre (**"SIAC"**) under the SIAC Arbitration Rules in force at the time the notice of arbitration is submitted, which rules are deemed to be incorporated by reference into this Section.

16.4 The seat of arbitration is Singapore. There will be one arbitrator. The language of the arbitration is English.

16.5 **Individual proceedings.** Each claim must be brought in an individual capacity. To the extent permitted by the SIAC Rules, the arbitrator may not consolidate the claims of more than one Creator without the consent of every affected Creator and may not hear or determine any claim on a class, collective, or representative basis.

16.6 **Interim relief from courts.** Either party may seek interim, injunctive, or other equitable relief from any court of competent jurisdiction (a) before an arbitral tribunal is constituted, or (b) to protect intellectual property or confidential information.

16.7 The United Nations Convention on Contracts for the International Sale of Goods does not apply.

## 16A. Reporting Illegal Content

16A.1 Any person may notify us of Content they believe to be illegal or to infringe their rights by sending an email to the address in Section 18. A valid notice should include (a) a sufficient explanation of why the Content is alleged to be illegal or infringing, (b) the precise on-chain location of the Content (Root NFT or License NFT mint address), (c) the notifier's name and contact details, and (d) a statement that the notifier is the holder of the right concerned (or is duly authorised by the holder) and has a good-faith belief in the accuracy of the notice.

16A.2 We aim to acknowledge receipt of a valid notice within seven (7) business days and, after review, notify the notifier and the affected Creator of the action we take, with a statement of reasons. This Section is the sole notice channel for reports of illegal Content. For general support or to report a bug, you may use the same email address in Section 18.

## 17. General

17.1 **Entire agreement.** These Terms, together with the documents they incorporate by reference, are the entire agreement between you and us about the subject matter and supersede any prior agreement or understanding.

17.2 **Severability.** If a court or arbitrator finds any provision unenforceable, the rest of these Terms remain in effect, and the unenforceable provision is severed only to the extent necessary to make the rest enforceable.

17.3 **No waiver.** If we do not enforce a provision, that is not a waiver of our right to enforce it or any other provision later.

17.4 **Assignment.** You may not assign these Terms without our prior written consent. We may assign these Terms (a) to a successor in a bona fide corporate reorganisation, merger, or sale of substantially all assets relating to the RootLens service, including on incorporation of a successor entity by the operator, provided that (i) the successor expressly assumes all of our obligations under these Terms in writing, (ii) the successor is bound by the Authoritative Hash version of these Terms applicable to each Root NFT, and (iii) we give you at least thirty (30) days' prior written notice through the RootLens Application (or, where the assignment is part of a wind-down under Section 15.3, the 90-day notice under that Section applies); or (b) where required by law. If you are a Consumer to whom Section 2A applies and you object to an assignment under (a) within the applicable notice period, you may terminate these Terms in respect of future submissions and revoke our authority under Section 6 (per Section 6.6); the assignment remains effective in respect of Root NFTs already minted, which continue to be governed by their Effective Version.

17.5 **Independent parties.** Except as expressly stated in Section 6, nothing in these Terms creates any partnership, joint venture, or employment relationship between you and us.

17.6 **Force majeure.** Neither party is liable for failure to perform an obligation if the failure is caused by an event beyond its reasonable control, including acts of God, war, terrorism, civil unrest, government action, network outage, or failure of any third-party infrastructure (including the Solana blockchain).

17.7 **Notices.** Notices to us must be sent to the contact address in Section 18. We may give notice to you through the RootLens Application or to any contact address you have given us.

17.8 **Language.** These Terms are made in English. Any translation is provided for convenience only. The English version controls in case of conflict, except as stated in Section 2A.8 for Japanese Consumers.

17.9 **Authoritative Hash.**

   (a) The hashed text starts at the first byte of the file and ends at the line-feed (`0x0A`) that terminates the line containing the Markdown text `**End of authoritative text**` below, inclusive.

   (b) Before hashing, line endings within the hashed text are normalised to single line-feed (`0x0A`) characters; trailing spaces and tabs on each line are stripped; the single line-feed terminating the line containing `**End of authoritative text**` is preserved. The Authoritative Hash value itself is never part of the hashed text.

   (c) The Authoritative Hash is published in the URL of the canonical copy of these Terms, in the form `https://rootlens.io/tos/v1.0.0/{Authoritative Hash}.txt`, and built into the TEE software so that the attestation always records which version applied to your submission. If any copy of this document, when hashed by the procedure in this Section, produces a different value, that copy is not these Terms.

   (d) **What you see is what binds.** The RootLens Application must show you the exact canonical text identified by the Authoritative Hash. If by mistake the Application shows you a different version and you agreed in good faith, the version you actually saw controls between you and us — but only to the extent that doesn't expand our rights or shrink yours below what the canonical text provides. Any mismatch is our problem, not yours.

## 18. Operator and Contact

The operator of the RootLens service ("we", "us") is:

> Yudai Mori (森 雄大), a Japanese sole proprietor trading as "moodai" (屋号).
> Email: `yudai.mori@moodai.jp`

The email address above is the single point of contact for all matters arising under these Terms, including (a) notices under Sections 16.2 and 17.7, (b) reports of illegal Content under Section 16A, and (c) data-protection requests under Section 13.5.

The operator's registered business address, telephone number, and other particulars required by the Japanese Act on Specified Commercial Transactions (特定商取引法 第11条) are published at `https://rootlens.io/legal/tokushoho` and form part of these Terms by reference where that Act applies. Consistent with Section 3.5, the operator has not appointed a GDPR Article 27 representative.

The Privacy Policy referenced in Section 13 is published at `https://rootlens.io/privacy-policy`.

The operator currently trades under a Japanese sole proprietorship and intends to incorporate a successor entity in the future. Section 17.4 (Assignment) governs the transition on incorporation.

---

**End of authoritative text**
