package org.silvermine.plugin.connectivity

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidConnectivityMapperTest {
   @Test
   fun `test that internet capability is required for connected status`() {
      assertTrue(AndroidConnectivityMapper.isConnected(hasInternet = true))
      assertFalse(AndroidConnectivityMapper.isConnected(hasInternet = false))
   }

   @Test
   fun `test that unmetered capability reports unmetered status`() {
      assertFalse(
         AndroidConnectivityMapper.isMetered(
            hasNotMetered = true,
            hasTemporarilyNotMetered = false
         )
      )
      assertFalse(
         AndroidConnectivityMapper.isMetered(
            hasNotMetered = true,
            hasTemporarilyNotMetered = true
         )
      )
   }

   @Test
   fun `test that temporarily not metered capability reports unmetered status`() {
      assertFalse(
         AndroidConnectivityMapper.isMetered(
            hasNotMetered = false,
            hasTemporarilyNotMetered = true
         )
      )
   }

   @Test
   fun `test that absence of unmetered capabilities reports metered status`() {
      assertTrue(
         AndroidConnectivityMapper.isMetered(
            hasNotMetered = false,
            hasTemporarilyNotMetered = false
         )
      )
   }

   @Test
   fun `test that unvalidated networks report constrained status`() {
      assertTrue(
         AndroidConnectivityMapper.isConstrained(
            isValidated = false,
            isBackgroundRestricted = false,
            isMetered = false
         )
      )
      assertTrue(
         AndroidConnectivityMapper.isConstrained(
            isValidated = false,
            isBackgroundRestricted = true,
            isMetered = true
         )
      )
   }

   @Test
   fun `test that background restrictions constrain metered networks`() {
      assertTrue(
         AndroidConnectivityMapper.isConstrained(
            isValidated = true,
            isBackgroundRestricted = true,
            isMetered = true
         )
      )
   }

   @Test
   fun `test that background restrictions do not constrain unmetered networks`() {
      assertFalse(
         AndroidConnectivityMapper.isConstrained(
            isValidated = true,
            isBackgroundRestricted = true,
            isMetered = false
         )
      )
   }

   @Test
   fun `test that metered networks are unconstrained without background restrictions`() {
      assertFalse(
         AndroidConnectivityMapper.isConstrained(
            isValidated = true,
            isBackgroundRestricted = false,
            isMetered = true
         )
      )
      assertFalse(
         AndroidConnectivityMapper.isConstrained(
            isValidated = true,
            isBackgroundRestricted = false,
            isMetered = false
         )
      )
   }

   @Test
   fun `test that wifi transport is preferred when available`() {
      assertEquals(
         ConnectionType.WIFI,
         AndroidConnectivityMapper.connectionType(
            hasWifi = true,
            hasEthernet = true,
            hasCellular = true
         )
      )
   }

   @Test
   fun `test that ethernet transport is used when wifi is unavailable`() {
      assertEquals(
         ConnectionType.ETHERNET,
         AndroidConnectivityMapper.connectionType(
            hasWifi = false,
            hasEthernet = true,
            hasCellular = true
         )
      )
   }

   @Test
   fun `test that cellular transport is used when wifi and ethernet are unavailable`() {
      assertEquals(
         ConnectionType.CELLULAR,
         AndroidConnectivityMapper.connectionType(
            hasWifi = false,
            hasEthernet = false,
            hasCellular = true
         )
      )
   }

   @Test
   fun `test that unknown transport is used when no known transport is available`() {
      assertEquals(
         ConnectionType.UNKNOWN,
         AndroidConnectivityMapper.connectionType(
            hasWifi = false,
            hasEthernet = false,
            hasCellular = false
         )
      )
   }
}
