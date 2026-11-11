{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (nub, sort, takeWhile)
import Data.Word (Word16, Word32, Word64)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import Replacement.RecordStore.CaseJson (decodeCase, encodeCase)
import Replacement.RecordStore.Cases
  ( CaseName (..)
  , Expected (..)
  , Operation (..)
  , OracleCase (..)
  , Registration (..)
  , Source (..)
  , callbackSize
  , encodeCallback
  , encodeRequest
  , mediumOf
  , model
  , requestSize
  )
import Replacement.RecordStore.Generate
import Replacement.RecordStore.Hardware (hardware)
import Replacement.RecordStore.Model

assert :: String -> Bool -> IO ()
assert message condition = unless condition (error message)

configuration :: Word64 -> Int -> Int -> GenerationConfig
configuration seed count maxOperations = either (error . show) id (mkGenerationConfig (GenerationSeed seed) count maxOperations)

corpus :: Word64 -> Int -> Int -> [ValidCase]
corpus seed count maxOperations = generateCases (configuration seed count maxOperations)

isRight :: Either a b -> Bool
isRight (Right _) = True
isRight _ = False

semanticInputs :: ValidCase -> (Kind, Medium, [Operation], Callback, Fault)
semanticInputs valid =
  let candidate = oracleCase valid
  in (kind candidate, initialMedium candidate, operations candidate, callbacks candidate, injection candidate)

expectRefusal :: String -> (DomainError -> Bool) -> Either DomainError a -> IO ()
expectRefusal label predicate result = case result of
  Left problem | predicate problem -> pure ()
  Left problem -> error (label ++ ": unexpected refusal " ++ show problem)
  Right _ -> error (label ++ ": accepted")

main :: IO ()
main = do
  determinismTests
  goldenTests
  constructionTests
  coverageTests
  boundaryTests
  refusalTests
  codecTests
  putStrLn "record-store counterexample generation tests passed"

determinismTests :: IO ()
determinismTests = do
  let first = corpus 0x1234 16 8
      second = corpus 0x1234 16 8
      other = corpus 0x1235 16 8
  assert "same seed and configuration reproduce the same cases" (first == second)
  assert "a different seed changes the semantic inputs" (map semanticInputs first /= map semanticInputs other)
  forM_ [(1, 3), (7, 6), (64, 12)] $ \(count, maxOperations) -> do
    let generated = corpus 0x99 count maxOperations
    assert ("exact case count " ++ show count) (length generated == count)
    forM_ generated $ \valid ->
      assert "every generated case passes the smart constructor" (isRight (validCase (oracleCase valid)))
  assert "case count below one refuses" (mkGenerationConfig (GenerationSeed 1) 0 5 == Left (InvalidCaseCount 0))
  assert "case count above sixty-four refuses" (mkGenerationConfig (GenerationSeed 1) 65 5 == Left (InvalidCaseCount 65))
  assert "operation bound below three refuses" (mkGenerationConfig (GenerationSeed 1) 1 2 == Left (InvalidOperationBound 2))
  assert "operation bound above twelve refuses" (mkGenerationConfig (GenerationSeed 1) 1 13 == Left (InvalidOperationBound 13))

-- The digest pins the exact corpus a seed and configuration produce, so a change
-- to generation is visible even when every other property still holds.
goldenTests :: IO ()
goldenTests = do
  let samples = corpus 0x1234 8 6
      digest = sha256Hex (sha256 (B.concat (map encodeCase samples)))
  assert ("fixed-seed corpus digest is unchanged: " ++ digest) (digest == "024dd74f47dedc1ae0bbe5b0defb093f0df3b5eafa1a99f43ef88ad4f71b0917")

constructionTests :: IO ()
constructionTests = do
  let generated = map oracleCase (corpus 0x7f4 24 12)
  assert "every generated case is its own validated form"
    (all (\valid -> validCase (oracleCase valid) == Right valid) (corpus 0x7f4 24 12))
  assert "no generated case is the init-only fallback" (all (\candidate -> length (operations candidate) > 2) generated)
  assert "no generated case drops its planned body" (all (not . null . body) generated)
  assert "the longest planned body survives the full operation bound" (maximum (map (length . body) generated) == 10)
  assert "the longest planned body survives a shorter operation bound"
    (maximum (map (length . body . oracleCase) (corpus 0x7f4 24 6)) == 4)
  assert "every case in the corpus is distinct"
    (length (nub (map semanticInputs (corpus 0x7f4 24 12))) == 24)

coverageTests :: IO ()
coverageTests = do
  let generated = map oracleCase (corpus 0xabc 64 12)
      states = concatMap bankStates generated
  assert "all five kinds appear" (sort (nub (map kind generated)) == [Kind0 .. Kind4])
  assert "all supported callbacks appear" (all (\chosen -> chosen `elem` map callbacks generated) [NoCallback, MarkerCallback, CorrectingCallback])
  assert "append appears" (any (any isAppend . operations) generated)
  assert "staging append appears" (any (any isStagingAppend . operations) generated)
  assert "read-at appears" (any (any isReadAt . operations) generated)
  assert "collect appears" (any (any isCollect . operations) generated)
  assert "erase-all appears" (any (any isEraseAll . operations) generated)
  assert "close and re-register appear" (any (\candidate -> Close `elem` body candidate && Register Normal `elem` body candidate) generated)
  assert "empty banks appear" (any (== 0) states)
  assert "partial banks appear" (any isPartial (zip (concatMap (replicate 2 . kind) generated) states))
  assert "full banks appear" (any isFull (zip (concatMap (replicate 2 . kind) generated) states))
  assert "wrap predecessor 0x7fff appears" (any (hasSequence 0x7fff) generated)
  assert "reads in bank one appear" (any (\(position, _) -> position >= fromIntegral blockBytes) (readOps generated))
  assert "a read at the last byte appears" (any (\(position, _) -> position == fromIntegral (2 * blockBytes - 1)) (readOps generated))
  assert "a read crossing a bank boundary appears" (any (\(position, lengthBytes) -> fromIntegral position `mod` blockBytes + lengthBytes > blockBytes) (readOps generated))
  assert "a bank-past boundary read appears" (any (\(position, _) -> position == fromIntegral (2 * blockBytes)) (readOps generated))
  assert "a rejected record marker appears" (any (\candidate -> any (hasRejectedRecord candidate) [Bank0, Bank1]) generated)
  assert "callback journal is exercised" (any (\candidate -> not (null (callbackEvents (expectedStore (model hardware candidate))))) generated)
  forM_ generated $ \candidate -> do
    let expected = model hardware candidate
        p = persistence (expectedStore expected)
        requestBytes = sum (map (B.length . encodeRequest) (requests p))
        callbackBytes = sum (map (B.length . encodeCallback) (callbackEvents (expectedStore expected)))
    assert "generated request journal fits the native request region" (requestBytes <= requestSize)
    assert "generated callback journal fits the native callback region" (callbackBytes <= callbackSize)

boundaryTests :: IO ()
boundaryTests = do
  let first = oracleCase (head (corpus 0x1234 8 12))
      g = geometry (kind first)
      firstBank = bankBytes g Bank0 (initialMedium first)
      expected = model hardware first
  assert "first generated case is the noisy wrap predecessor" (sequenceOf (B.take 2 firstBank) == Sequence 0x7fff)
  assert "first generated case appends" (any isAppend (operations first))
  assert "first generated case stays small" (length (operations first) <= 5)
  assert "stock append after 0x7fff journals the masked successor" (any maskedSuccessor (requests (persistence (expectedStore expected))))
  forM_ [Kind0 .. Kind4] $ \kind' -> do
    assert "empty banks are valid" (isRight (validCase (emptyCase kind')))
    assert "full banks are valid" (isRight (validCase (fullCase kind')))
  let g0 = geometry Kind0
      overfull = mediumOf g0 [(fromIntegral p, False) | p <- [0 .. recordCount g0]] []
      readCase operations' = OracleCase (CaseName "gen-test-read") Kind0 (mediumOf (geometry Kind0) [] []) ([Open, Register Normal] ++ operations') NoCallback NoFault
      readStore = expectedStore (model hardware (readCase []))
      caller = B.replicate 16 0xa5
      (lastStatus, lastBytes) = readAt (BytePosition (fromIntegral (2 * blockBytes) - 1)) 1 caller readStore
      (pastStatus, pastBytes) = readAt (BytePosition (fromIntegral (2 * blockBytes))) 1 caller readStore
      (crossStatus, crossBytes) = readAt (BytePosition (fromIntegral blockBytes - 1)) 2 caller readStore
  expectRefusal "an occupied prefix longer than the geometry refuses" isBankContents
    (validCase ((emptyCase Kind0) { initialMedium = overfull }))
  assert "a read at the last byte is valid" (isRight (validCase (readCase [ReadAt (BytePosition (fromIntegral (2 * blockBytes - 1))) 1])))
  assert "a read crossing a bank boundary is valid" (isRight (validCase (readCase [ReadAt (BytePosition (fromIntegral (blockBytes - 1))) 2])))
  assert "a read at the bank-past boundary is a valid case" (isRight (validCase (readCase [ReadAt (BytePosition (fromIntegral (2 * blockBytes))) 1])))
  expectRefusal "a read one byte past the bank-past boundary refuses" isReadBounds
    (validCase (readCase [ReadAt (BytePosition (fromIntegral (2 * blockBytes) + 1)) 1]))
  assert "the last byte of both banks reads successfully" (lastStatus == 0 && B.take 1 lastBytes == B.singleton 0xff)
  assert "the bank-past boundary read is refused by the operation" (pastStatus == statusInvalid && B.take 1 pastBytes == B.singleton 0)
  assert "a crossing read is refused by the operation" (crossStatus == statusInvalid && B.take 2 crossBytes == B.replicate 2 0)
  where
    maskedSuccessor (Program _ _ payload) = sequenceOf payload == Sequence 0
    maskedSuccessor _ = False
    emptyCase kind' = OracleCase (CaseName "gen-test-empty") kind' (mediumOf (geometry kind') [] []) [Open, Register Normal] NoCallback NoFault
    fullCase kind' =
      let g' = geometry kind'
          full = [(fromIntegral p, False) | p <- [0 .. recordCount g' - 1]]
      in OracleCase (CaseName "gen-test-full") kind' (mediumOf g' full full) [Open, Register Normal, Latest] NoCallback NoFault

refusalTests :: IO ()
refusalTests = do
  assert "the base fixture is valid" (isRight (validCase (base [Open, Register Normal, Latest] NoCallback)))
  assert "a prefix-only case is valid" (isRight (validCase (base [Open, Register Normal] NoCallback)))
  assert "close then register is valid" (isRight (validCase (base [Open, Register Normal, Close, Register Normal, Latest] NoCallback)))
  expectRefusal "a missing prefix refuses" isMissingPrefix (validCase (base [Register Normal, Latest] NoCallback))
  expectRefusal "a body open refuses" isUnsupportedOperation (validCase (base [Open, Register Normal, Open] NoCallback))
  expectRefusal "a cache boundary refuses" isUnsupportedOperation (validCase (base [Open, Register Normal, CacheBoundary 0 0 0] NoCallback))
  expectRefusal "a kind selection refuses" isUnsupportedOperation (validCase (base [Open, Register Normal, On Kind0 Latest] NoCallback))
  expectRefusal "a non-normal registration refuses" isUnsupportedRegistration (validCase (base [Open, Register Normal, Register OddOffset] NoCallback))
  expectRefusal "registering while active refuses" isLifecycle (validCase (base [Open, Register Normal, Register Normal] NoCallback))
  expectRefusal "closing while inactive refuses" isLifecycle (validCase (base [Open, Register Normal, Close, Close] NoCallback))
  expectRefusal "latest while inactive refuses" isLifecycle (validCase (base [Open, Register Normal, Close, Latest] NoCallback))
  expectRefusal "append while inactive refuses" isLifecycle (validCase (base [Open, Register Normal, Close, Append (Seed 1) CallerSource] NoCallback))
  expectRefusal "append seed ffff refuses" isInvalidSequence (validCase (base [Open, Register Normal, Append (Seed 0xffff) CallerSource] NoCallback))
  expectRefusal "a hardware callback refuses" isUnsupportedCallback (validCase (base [Open, Register Normal] Hardware2))
  expectRefusal "an injected fault refuses" isUnsupportedFault
    (validCase ((base [Open, Register Normal] NoCallback) { injection = FailRequest (PersistenceStep 0) }))
  expectRefusal "an invalid native case id refuses" isInvalidName
    (validCase ((base [Open, Register Normal] NoCallback) { name = CaseName "Bad Name" }))
  let g0 = geometry Kind0
  expectRefusal "missing banks refuse" isBankGeometry
    (validCase ((base [Open, Register Normal] NoCallback) { initialMedium = Medium M.empty }))
  expectRefusal "a bank of the wrong size refuses" isBankGeometry
    (validCase ((base [Open, Register Normal] NoCallback)
      { initialMedium = putBank g0 Bank1 (B.replicate blockBytes 0xff) (putBank g0 Bank0 (B.replicate 10 0xff) (Medium M.empty)) }))
  expectRefusal "an extra medium block refuses" isBankGeometry
    (validCase ((base [Open, Register Normal] NoCallback)
      { initialMedium = case mediumOf g0 [] [] of Medium blocks -> Medium (M.insert (PoolBlock 0x99) (B.replicate blockBytes 0xff) blocks) }))
  expectRefusal "occupied records after an erased record refuse" isBankContents
    (validCase ((base [Open, Register Normal] NoCallback)
      { initialMedium = mediumOf g0 [(10, False), (0xffff, False), (11, False)] [] }))
  expectRefusal "a zero-length read refuses" isReadBounds (validCase (base [Open, Register Normal, ReadAt (BytePosition 0) 0] NoCallback))
  expectRefusal "an oversized read refuses" isReadBounds
    (validCase (base [Open, Register Normal, ReadAt (BytePosition 0) (recordBytes g0 + 1)] NoCallback))
  expectRefusal "a read past both banks refuses" isReadBounds
    (validCase (base [Open, Register Normal, ReadAt (BytePosition 0x40001) 1] NoCallback))
  expectRefusal "too many operation slots refuse" isEventCapacity
    (validCase (base ([Open, Register Normal] ++ replicate 14 Latest) NoCallback))
  let rejectedRecords = [(fromIntegral p, True) | p <- [0 .. recordCount g0 - 1]]
      overflowing = OracleCase (CaseName "gen-test-callback-overflow") Kind0 (mediumOf g0 rejectedRecords rejectedRecords) [Open, Register Normal, Latest] MarkerCallback NoFault
  expectRefusal "an oversized callback journal refuses" isCallbackOverflow (validCase overflowing)
  where
    base operations' callback' = OracleCase (CaseName "gen-test-base") Kind0 (mediumOf (geometry Kind0) [] []) operations' callback' NoFault

codecTests :: IO ()
codecTests = do
  let samples = corpus 0x55 16 8
      sample = head samples
      encoded = encodeCase sample
      value = either error id (eitherDecodeStrict' encoded)
      mutated key replacementValue = case value of
        Object fields -> encodeCanonical (Object (KM.insert key replacementValue fields))
        _ -> error "encoded case is not an object"
      deleted key = case value of
        Object fields -> encodeCanonical (Object (KM.delete key fields))
        _ -> error "encoded case is not an object"
      known = either (error . show) id (validCase (OracleCase (CaseName "gen-test-codec") Kind0
        (mediumOf (geometry Kind0) [(0x1234, False)] [(0x00ff, False)]) [Open, Register Normal] NoCallback NoFault))
      knownEncoded = encodeCase known
      banksPrefix = "{\"banks\":{\"bank0\":\""
      digitOffset = B.length banksPrefix
      replacement = if B.index knownEncoded digitOffset == 0x30 then 0x31 else 0x30
      corrupted = B.take digitOffset knownEncoded <> B.singleton replacement <> B.drop (digitOffset + 1) knownEncoded
  forM_ samples $ \generated ->
    assert "every generated case roundtrips through the codec" (decodeCase (encodeCase generated) == Right generated)
  assert "the codec roundtrip preserves the case" (decodeCase encoded == Right sample)
  assert "re-encoding a decoded case is canonical" (either (const False) (\generated -> encodeCase generated == encoded) (decodeCase encoded))
  assert "case JSON carries only typed input fields" $ case value of
    Object fields -> sort (map K.toText (KM.keys fields)) == sort ["version", "name", "kind", "banks", "operations", "callbacks", "fault"]
    _ -> False
  assert "encoded case starts with canonical lowercase bank hex" (B.take digitOffset knownEncoded == banksPrefix)
  assert "a corrupted bank digit changes the decoded sequence"
    (case decodeCase corrupted of
       Right decoded ->
         let g' = geometry (kind (oracleCase decoded))
         in sequenceOf (bankBytes g' Bank0 (initialMedium (oracleCase decoded))) == Sequence 0x1204
       Left _ -> False)
  expectRefusal "an unsupported version refuses" isVersion (decodeCase (mutated "version" (toJSON (2 :: Int))))
  expectRefusal "an unknown field refuses" isMalformed (decodeCase (mutated "expected" (object [])))
  expectRefusal "a missing field refuses" isMalformed (decodeCase (deleted "banks"))
  expectRefusal "an unknown operation refuses" isMalformed
    (decodeCase (mutated "operations" (toJSON [object ["op" .= ("teleport" :: String)]])))
  expectRefusal "an unsupported callback refuses" isMalformed (decodeCase (mutated "callbacks" (String "hardware2")))
  expectRefusal "an unsupported kind refuses" isMalformed (decodeCase (mutated "kind" (toJSON (9 :: Int))))
  expectRefusal "invalid bank hex refuses" isMalformed
    (decodeCase (mutated "banks" (object ["bank0" .= ("zz" :: String), "bank1" .= ("00" :: String)])))
  expectRefusal "uppercase bank hex refuses" isMalformed
    (decodeCase (mutated "banks" (object ["bank0" .= ("FF" :: String), "bank1" .= ("" :: String)])))
  expectRefusal "odd-length bank hex refuses" isMalformed
    (decodeCase (mutated "banks" (object ["bank0" .= ("abc" :: String), "bank1" .= ("" :: String)])))
  expectRefusal "a short bank refuses" isBankGeometry
    (decodeCase (mutated "banks" (object ["bank0" .= ("" :: String), "bank1" .= ("" :: String)])))
  expectRefusal "an encoded lifecycle violation refuses" isLifecycle
    (decodeCase (mutated "operations" (toJSON
      [ object ["op" .= ("open" :: String)]
      , object ["op" .= ("register" :: String), "mode" .= ("normal" :: String)]
      , object ["op" .= ("close" :: String)]
      , object ["op" .= ("close" :: String)]
      ])))
  expectRefusal "truncated JSON refuses" isMalformed (decodeCase "{")
  expectRefusal "a duplicate JSON key refuses" isMalformed (decodeCase "{\"version\":1,\"version\":1}")
  expectRefusal "a fractional JSON integer refuses" isMalformed (decodeCase "{\"version\":1.0}")
  expectRefusal "empty input refuses" isMalformed (decodeCase "")
  where
    isVersion (UnsupportedCaseVersion 2) = True
    isVersion _ = False

bankStates :: OracleCase -> [Int]
bankStates candidate = [occupiedPrefix (kind candidate) (bankBytes g bank' (initialMedium candidate)) | bank' <- [Bank0, Bank1]]
  where
    g = geometry (kind candidate)

occupiedPrefix :: Kind -> B.ByteString -> Int
occupiedPrefix kind' bytes = length (takeWhile occupied [0 .. recordCount g - 1])
  where
    g = geometry kind'
    occupied p = sequenceOf (B.take 2 (B.drop (p * recordBytes g) bytes)) /= Sequence 0xffff

isPartial :: (Kind, Int) -> Bool
isPartial (kind', count) = count > 0 && count < recordCount (geometry kind')

isFull :: (Kind, Int) -> Bool
isFull (kind', count) = count == recordCount (geometry kind')

hasSequence :: Word16 -> OracleCase -> Bool
hasSequence target candidate = any bankHas [Bank0, Bank1]
  where
    g = geometry (kind candidate)
    bankHas bank' = any (\p -> sequenceOf (B.take 2 (B.drop (p * recordBytes g) (bankBytes g bank' (initialMedium candidate)))) == Sequence target) [0 .. recordCount g - 1]

hasRejectedRecord :: OracleCase -> Bank -> Bool
hasRejectedRecord candidate bank' = any rejected (occupiedRecords candidate bank')
  where
    g = geometry (kind candidate)
    bytes = bankBytes g bank' (initialMedium candidate)
    rejected p = B.index bytes (p * recordBytes g + 2) == 0xee

occupiedRecords :: OracleCase -> Bank -> [Int]
occupiedRecords candidate bank' =
  [p | p <- [0 .. recordCount g - 1], sequenceOf (B.take 2 (B.drop (p * recordBytes g) bytes)) /= Sequence 0xffff]
  where
    g = geometry (kind candidate)
    bytes = bankBytes g bank' (initialMedium candidate)

readOps :: [OracleCase] -> [(Word32, Int)]
readOps generated = [(position, lengthBytes) | candidate <- generated, ReadAt (BytePosition position) lengthBytes <- operations candidate]

body :: OracleCase -> [Operation]
body = drop 2 . operations

isAppend :: Operation -> Bool
isAppend (Append _ _) = True
isAppend _ = False

isStagingAppend :: Operation -> Bool
isStagingAppend (Append _ StagingSource) = True
isStagingAppend _ = False

isReadAt :: Operation -> Bool
isReadAt (ReadAt _ _) = True
isReadAt _ = False

isCollect :: Operation -> Bool
isCollect Collect = True
isCollect _ = False

isEraseAll :: Operation -> Bool
isEraseAll EraseAll = True
isEraseAll _ = False

isMissingPrefix :: DomainError -> Bool
isMissingPrefix MissingPrefix = True
isMissingPrefix _ = False

isUnsupportedOperation :: DomainError -> Bool
isUnsupportedOperation (UnsupportedOperation _) = True
isUnsupportedOperation _ = False

isUnsupportedRegistration :: DomainError -> Bool
isUnsupportedRegistration (UnsupportedRegistration _) = True
isUnsupportedRegistration _ = False

isUnsupportedCallback :: DomainError -> Bool
isUnsupportedCallback (UnsupportedCallback _) = True
isUnsupportedCallback _ = False

isUnsupportedFault :: DomainError -> Bool
isUnsupportedFault (UnsupportedFault _) = True
isUnsupportedFault _ = False

isLifecycle :: DomainError -> Bool
isLifecycle (LifecycleViolation _) = True
isLifecycle _ = False

isInvalidSequence :: DomainError -> Bool
isInvalidSequence (InvalidSequence _) = True
isInvalidSequence _ = False

isBankGeometry :: DomainError -> Bool
isBankGeometry (InvalidBankGeometry _) = True
isBankGeometry _ = False

isBankContents :: DomainError -> Bool
isBankContents (InvalidBankContents _) = True
isBankContents _ = False

isReadBounds :: DomainError -> Bool
isReadBounds (InvalidReadBounds _) = True
isReadBounds _ = False

isEventCapacity :: DomainError -> Bool
isEventCapacity (EventCapacityExceeded _) = True
isEventCapacity _ = False

isCallbackOverflow :: DomainError -> Bool
isCallbackOverflow (CallbackJournalOverflow _) = True
isCallbackOverflow _ = False

isInvalidName :: DomainError -> Bool
isInvalidName (InvalidCaseName _) = True
isInvalidName _ = False

isMalformed :: DomainError -> Bool
isMalformed (MalformedCaseJson _) = True
isMalformed _ = False
